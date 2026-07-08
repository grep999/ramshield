#!/usr/bin/env python3
"""
RamShield extreme real-time attack simulator.

Subcommands for burst/flood/phase attacks, plus an interactive REPL to
control attacks while RamShield is running.

Examples:
  ./scripts/attack_extreme.py burst --events 500000 --workers 256
  ./scripts/attack_extreme.py flood --duration 120 --workers 512 --batch-size 2000
  ./scripts/attack_extreme.py phase --plan volumetric,subnet,botnet
  ./scripts/attack_extreme.py interactive --workers 128
  ./scripts/attack_extreme.py exec flood duration=60 mode=volumetric target=10.0.0.1
"""

from __future__ import annotations

import argparse
import json
import random
import shlex
import socket
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass, field
from typing import Callable, Dict, Iterable, List, Optional, Tuple

DEFAULT_HOST = "127.0.0.1"
DEFAULT_PORT = 7890


# ── Shared state ──────────────────────────────────────────────────────────────

@dataclass
class AttackConfig:
    host: str = DEFAULT_HOST
    port: int = DEFAULT_PORT
    workers: int = 256
    batch_size: int = 2000
    mode: str = "mixed"
    target_ip: str = "10.255.0.1"
    read_response: bool = False
    reconnect: bool = True
    socket_buffer: int = 1 << 20  # 1 MiB send buffer


@dataclass
class LiveStats:
    sent_events: int = 0
    sent_batches: int = 0
    errors: int = 0
    reconnects: int = 0
    lock: threading.Lock = field(default_factory=threading.Lock)
    started: float = field(default_factory=time.perf_counter)

    def add(self, events: int = 0, batches: int = 0, errors: int = 0, reconnects: int = 0) -> None:
        with self.lock:
            self.sent_events += events
            self.sent_batches += batches
            self.errors += errors
            self.reconnects += reconnects

    def snapshot(self) -> Tuple[int, int, int, int, float]:
        with self.lock:
            elapsed = max(time.perf_counter() - self.started, 1e-9)
            return self.sent_events, self.sent_batches, self.errors, self.reconnects, elapsed

    def reset(self) -> None:
        with self.lock:
            self.sent_events = 0
            self.sent_batches = 0
            self.errors = 0
            self.reconnects = 0
            self.started = time.perf_counter()


class StopFlag:
    def __init__(self) -> None:
        self._ev = threading.Event()

    def stop(self) -> None:
        self._ev.set()

    def stopped(self) -> bool:
        return self._ev.is_set()

    def clear(self) -> None:
        self._ev.clear()


# ── Traffic generation ────────────────────────────────────────────────────────

MODES = ("mixed", "volumetric", "botnet", "subnet", "errors", "synwave", "multi_subnet")


def make_ip(cfg: AttackConfig, rng: random.Random, worker_id: int) -> str:
    mode = cfg.mode
    if mode == "volumetric":
        return cfg.target_ip
    if mode == "subnet":
        return f"203.0.113.{rng.randint(1, 254)}"
    if mode == "multi_subnet":
        prefix = rng.choice([(198, 51, 100), (203, 0, 113), (192, 0, 2), (100, 64, 0)])
        return f"{prefix[0]}.{prefix[1]}.{prefix[2]}.{rng.randint(1, 254)}"
    if mode == "botnet":
        return f"{rng.randint(1, 223)}.{rng.randint(0, 255)}.{rng.randint(0, 255)}.{rng.randint(1, 254)}"
    if mode == "synwave":
        # Rapid rotation across small pool + one hot IP
        if rng.random() < 0.4:
            return cfg.target_ip
        return f"10.{worker_id % 256}.{rng.randint(0, 255)}.{rng.randint(1, 254)}"
    roll = rng.random()
    if roll < 0.2:
        return cfg.target_ip
    if roll < 0.45:
        return f"198.51.100.{rng.randint(1, 254)}"
    return f"{rng.randint(1, 223)}.{rng.randint(0, 255)}.{rng.randint(0, 255)}.{rng.randint(1, 254)}"


def make_event(cfg: AttackConfig, rng: random.Random, worker_id: int) -> dict:
    if cfg.mode == "errors":
        status = rng.choice([404, 403, 500, 502, 503])
    else:
        status = 200 if rng.random() > 0.03 else rng.choice([404, 500, 503])
    return {
        "ip": make_ip(cfg, rng, worker_id),
        "bytes": rng.randint(32, 8192),
        "status_code": status,
        "proto_fp": rng.randint(0, 0xFFFF),
    }


def gen_batch(cfg: AttackConfig, rng: random.Random, worker_id: int, size: int) -> List[dict]:
    return [make_event(cfg, rng, worker_id) for _ in range(size)]


# ── Network I/O ───────────────────────────────────────────────────────────────

def tune_socket(sock: socket.socket, buf: int) -> None:
    sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    try:
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, buf)
    except OSError:
        pass


def connect(cfg: AttackConfig) -> socket.socket:
    sock = socket.create_connection((cfg.host, cfg.port), timeout=10.0)
    tune_socket(sock, cfg.socket_buffer)
    return sock


def send_batch(sock: socket.socket, events: List[dict], read_response: bool) -> Optional[str]:
    line = json.dumps({"type": "report_connections", "events": events}, separators=(",", ":")) + "\n"
    sock.sendall(line.encode())
    if not read_response:
        return None
    sock.settimeout(1.0)
    buf = bytearray()
    while b"\n" not in buf:
        chunk = sock.recv(4096)
        if not chunk:
            break
        buf.extend(chunk)
    return bytes(buf).decode(errors="replace").strip()


def ipc_request(host: str, port: int, payload: dict, timeout: float = 3.0) -> dict:
    with socket.create_connection((host, port), timeout=timeout) as sock:
        tune_socket(sock, 65536)
        sock.sendall((json.dumps(payload) + "\n").encode())
        sock.settimeout(timeout)
        data = sock.recv(65536).decode(errors="replace").strip()
    return json.loads(data)


# ── Worker loops ──────────────────────────────────────────────────────────────

def flood_worker(
    worker_id: int,
    cfg: AttackConfig,
    stop: StopFlag,
    stats: LiveStats,
    duration: Optional[float],
    max_events: Optional[int],
) -> None:
    rng = random.Random(worker_id ^ int(time.time() * 1e9))
    deadline = time.perf_counter() + duration if duration else None
    local_sent = 0
    sock: Optional[socket.socket] = None

    def ensure_sock() -> socket.socket:
        nonlocal sock
        if sock is not None:
            return sock
        sock = connect(cfg)
        return sock

    while not stop.stopped():
        if deadline and time.perf_counter() >= deadline:
            break
        if max_events is not None and local_sent >= max_events:
            break

        batch_n = cfg.batch_size
        if max_events is not None:
            batch_n = min(batch_n, max_events - local_sent)
            if batch_n <= 0:
                break

        try:
            s = ensure_sock()
            batch = gen_batch(cfg, rng, worker_id, batch_n)
            send_batch(s, batch, cfg.read_response)
            stats.add(events=len(batch), batches=1)
            local_sent += len(batch)
        except OSError:
            stats.add(errors=1)
            if sock:
                try:
                    sock.close()
                except OSError:
                    pass
                sock = None
            if cfg.reconnect:
                stats.add(reconnects=1)
                time.sleep(0.001)
                continue
            break

    if sock:
        try:
            sock.close()
        except OSError:
            pass


def run_flood(
    cfg: AttackConfig,
    *,
    duration: Optional[float] = None,
    max_events: Optional[int] = None,
    stop: Optional[StopFlag] = None,
    stats: Optional[LiveStats] = None,
    progress: bool = True,
) -> LiveStats:
    stop = stop or StopFlag()
    stats = stats or LiveStats()
    stats.reset()

    threads: List[threading.Thread] = []
    for i in range(cfg.workers):
        t = threading.Thread(
            target=flood_worker,
            args=(i, cfg, stop, stats, duration, max_events),
            name=f"flood-{i}",
            daemon=True,
        )
        t.start()
        threads.append(t)

    if progress:
        _progress_loop(stop, stats, label="FLOOD")

    stop.stop()
    for t in threads:
        t.join(timeout=5.0)
    return stats


def run_burst(cfg: AttackConfig, events: int, progress: bool = True) -> LiveStats:
    per = events // cfg.workers
    rem = events % cfg.workers
    counts = [per + (1 if i < rem else 0) for i in range(cfg.workers)]
    stop = StopFlag()
    stats = LiveStats()
    stats.reset()

    def burst_one(wid: int, count: int) -> None:
        if count <= 0:
            return
        rng = random.Random(wid)
        sent = 0
        sock: Optional[socket.socket] = None
        while sent < count and not stop.stopped():
            try:
                if sock is None:
                    sock = connect(cfg)
                n = min(cfg.batch_size, count - sent)
                batch = gen_batch(cfg, rng, wid, n)
                send_batch(sock, batch, cfg.read_response)
                stats.add(events=n, batches=1)
                sent += n
            except OSError:
                stats.add(errors=1)
                if sock:
                    try:
                        sock.close()
                    except OSError:
                        pass
                    sock = None
                if not cfg.reconnect:
                    break
        if sock:
            try:
                sock.close()
            except OSError:
                pass

    threads = [
        threading.Thread(target=burst_one, args=(i, counts[i]), daemon=True)
        for i in range(cfg.workers)
    ]
    for t in threads:
        t.start()

    if progress:
        _progress_loop(stop, stats, label="BURST", target=events)

    for t in threads:
        t.join(timeout=30.0)
    return stats


def _progress_loop(stop: StopFlag, stats: LiveStats, label: str, target: Optional[int] = None) -> None:
    try:
        while True:
            sent, batches, errors, reconnects, elapsed = stats.snapshot()
            eps = sent / elapsed
            line = (
                f"\r[{label}] events={sent:,} batches={batches:,} "
                f"eps={eps:,.0f} err={errors} reconnects={reconnects} t={elapsed:.1f}s"
            )
            if target:
                line += f" target={target:,} ({100 * sent / target:.1f}%)"
            sys.stdout.write(line)
            sys.stdout.flush()
            if target and sent >= target:
                break
            if all(not t.is_alive() for t in threading.enumerate() if t.name.startswith("flood-")):
                # burst threads don't use flood- prefix; break on timer for flood via stop
                pass
            time.sleep(0.25)
    except KeyboardInterrupt:
        stop.stop()
    finally:
        sys.stdout.write("\n")


# ── Phase plans ───────────────────────────────────────────────────────────────

PHASE_PRESETS: Dict[str, List[Tuple[str, float, Optional[int]]]] = {
    # (mode, duration_sec, optional_burst_events)
    "default": [
        ("volumetric", 15.0, None),
        ("subnet", 20.0, None),
        ("botnet", 30.0, None),
        ("synwave", 20.0, None),
    ],
    "extreme": [
        ("volumetric", 30.0, None),
        ("multi_subnet", 45.0, None),
        ("botnet", 60.0, None),
        ("errors", 30.0, None),
        ("mixed", 60.0, None),
    ],
    "quick": [
        ("volumetric", 5.0, 50_000),
        ("subnet", 5.0, 50_000),
    ],
}


def run_phase(cfg: AttackConfig, plan_name: str, custom_modes: Optional[List[str]] = None) -> LiveStats:
    if custom_modes:
        plan = [(m, 10.0, None) for m in custom_modes]
    else:
        plan = PHASE_PRESETS.get(plan_name, PHASE_PRESETS["default"])

    total = LiveStats()
    total.reset()

    print(f"=== PHASE ATTACK plan={plan_name!r} workers={cfg.workers} ===")
    for i, (mode, dur, burst) in enumerate(plan, 1):
        cfg.mode = mode
        print(f"\n--- phase {i}/{len(plan)}: mode={mode} duration={dur}s burst={burst} ---")
        if burst:
            s = run_burst(cfg, burst, progress=True)
        else:
            stop = StopFlag()
            s = run_flood(cfg, duration=dur, stop=stop, progress=True)
        sent, batches, errors, reconnects, elapsed = s.snapshot()
        total.add(events=sent, batches=batches, errors=errors, reconnects=reconnects)
        print(f"phase done: {sent:,} events in {elapsed:.1f}s")

    ts, tb, te, tr, tel = total.snapshot()
    print(f"\n=== PHASE TOTAL: {ts:,} events, {te} errors, {tel:.1f}s ===")
    return total


# ── CLI helpers ───────────────────────────────────────────────────────────────

def cfg_from_ns(ns: argparse.Namespace) -> AttackConfig:
    return AttackConfig(
        host=getattr(ns, "host", DEFAULT_HOST),
        port=getattr(ns, "port", DEFAULT_PORT),
        workers=getattr(ns, "workers", 256),
        batch_size=getattr(ns, "batch_size", 2000),
        mode=getattr(ns, "mode", "mixed"),
        target_ip=getattr(ns, "target_ip", "10.255.0.1"),
        read_response=getattr(ns, "read_response", False),
        reconnect=not getattr(ns, "no_reconnect", False),
        socket_buffer=getattr(ns, "socket_buffer", 1 << 20),
    )


def print_results(stats: LiveStats, cfg: AttackConfig) -> None:
    sent, batches, errors, reconnects, elapsed = stats.snapshot()
    eps = sent / elapsed if elapsed > 0 else 0
    print("--- results ---")
    print(f"Target:      {cfg.host}:{cfg.port}")
    print(f"Events:      {sent:,}")
    print(f"Batches:     {batches:,}")
    print(f"Errors:      {errors}")
    print(f"Reconnects:  {reconnects}")
    print(f"Elapsed:     {elapsed:.2f}s")
    print(f"Throughput:  {eps:,.0f} events/s")
    print(f"Mode:        {cfg.mode}")
    print(f"Workers:     {cfg.workers}")
    print(f"Batch size:  {cfg.batch_size}")


def cli_stats(cfg: AttackConfig) -> None:
    try:
        resp = ipc_request(cfg.host, cfg.port, {"type": "get_stats"})
        print(json.dumps(resp, indent=2))
    except Exception as e:
        print(f"stats failed: {e}", file=sys.stderr)


def cli_check(cfg: AttackConfig, ip: str) -> None:
    try:
        resp = ipc_request(cfg.host, cfg.port, {"type": "check_ip", "ip": ip})
        print(json.dumps(resp, indent=2))
    except Exception as e:
        print(f"check failed: {e}", file=sys.stderr)


# ── exec: key=value command line ─────────────────────────────────────────────

def parse_kv_args(tokens: List[str]) -> Dict[str, str]:
    out: Dict[str, str] = {}
    for t in tokens:
        if "=" not in t:
            continue
        k, v = t.split("=", 1)
        out[k.strip()] = v.strip()
    return out


def apply_kv(cfg: AttackConfig, kv: Dict[str, str]) -> None:
    mapping = {
        "host": ("host", str),
        "port": ("port", int),
        "workers": ("workers", int),
        "batch": ("batch_size", int),
        "batch_size": ("batch_size", int),
        "mode": ("mode", str),
        "target": ("target_ip", str),
        "target_ip": ("target_ip", str),
    }
    for k, v in kv.items():
        if k not in mapping:
            print(f"warning: unknown key {k!r}", file=sys.stderr)
            continue
        attr, typ = mapping[k]
        setattr(cfg, attr, typ(v))


# ── Interactive REPL ──────────────────────────────────────────────────────────

REPL_HELP = """
Commands (real-time control):
  help                         Show this help
  status                       Live counters
  stats                        Query RamShield get_stats
  check <ip>                   Query RamShield check_ip
  burst <n>                    Send n events and wait
  flood [seconds]              Flood until seconds elapsed (default: 60)
  stop                         Stop running flood
  mode <name>                  mixed|volumetric|botnet|subnet|errors|synwave|multi_subnet
  target <ip>                  Set hot target IP
  workers <n>                  Parallel connections
  batch <n>                    Events per IPC message
  host <ip>                    Target host
  port <n>                     Target port
  phase [plan]                 Run phase plan (default|extreme|quick)
  reset                        Reset live counters
  quit / exit                  Leave REPL
"""


def repl(cfg: AttackConfig) -> int:
    stop = StopFlag()
    stats = LiveStats()
    flood_thread: Optional[threading.Thread] = None

    print("=== RamShield EXTREME interactive ===")
    print(f"Target {cfg.host}:{cfg.port} | workers={cfg.workers} batch={cfg.batch_size}")
    print("Type 'help' for commands.\n")

    while True:
        try:
            line = input("rs-attack> ").strip()
        except (EOFError, KeyboardInterrupt):
            print()
            break
        if not line:
            continue

        parts = shlex.split(line)
        cmd = parts[0].lower()
        args = parts[1:]

        if cmd in ("quit", "exit", "q"):
            stop.stop()
            break
        if cmd == "help":
            print(REPL_HELP)
            continue
        if cmd == "status":
            sent, batches, errors, reconnects, elapsed = stats.snapshot()
            print(
                f"events={sent:,} batches={batches:,} eps={sent/elapsed:,.0f} "
                f"errors={errors} reconnects={reconnects} mode={cfg.mode} target={cfg.target_ip}"
            )
            continue
        if cmd == "stats":
            cli_stats(cfg)
            continue
        if cmd == "check" and args:
            cli_check(cfg, args[0])
            continue
        if cmd == "mode" and args:
            if args[0] not in MODES:
                print(f"unknown mode; choose from: {', '.join(MODES)}")
            else:
                cfg.mode = args[0]
                print(f"mode={cfg.mode}")
            continue
        if cmd == "target" and args:
            cfg.target_ip = args[0]
            print(f"target={cfg.target_ip}")
            continue
        if cmd == "workers" and args:
            cfg.workers = max(1, int(args[0]))
            print(f"workers={cfg.workers}")
            continue
        if cmd == "batch" and args:
            cfg.batch_size = max(1, int(args[0]))
            print(f"batch_size={cfg.batch_size}")
            continue
        if cmd == "host" and args:
            cfg.host = args[0]
            print(f"host={cfg.host}")
            continue
        if cmd == "port" and args:
            cfg.port = int(args[0])
            print(f"port={cfg.port}")
            continue
        if cmd == "reset":
            stats.reset()
            print("counters reset")
            continue
        if cmd == "stop":
            stop.stop()
            if flood_thread and flood_thread.is_alive():
                flood_thread.join(timeout=10)
            print("stopped")
            continue
        if cmd == "burst":
            n = int(args[0]) if args else 100_000
            stop.clear()
            s = run_burst(cfg, n, progress=True)
            sent, _, _, _, _ = s.snapshot()
            stats.add(events=sent)
            print_results(s, cfg)
            continue
        if cmd == "flood":
            dur = float(args[0]) if args else 60.0
            stop.clear()
            stop_flag = StopFlag()

            def _run() -> None:
                run_flood(cfg, duration=dur, stop=stop_flag, stats=stats, progress=False)

            flood_thread = threading.Thread(target=_run, daemon=True)
            flood_thread.start()
            print(f"flood started {dur}s mode={cfg.mode} — use 'stop' or wait")
            t0 = time.perf_counter()
            try:
                while flood_thread.is_alive() and time.perf_counter() - t0 < dur + 2:
                    sent, _, _, _, elapsed = stats.snapshot()
                    sys.stdout.write(f"\r[flood] {sent:,} events eps={sent/max(elapsed,1e-9):,.0f}   ")
                    sys.stdout.flush()
                    time.sleep(0.2)
            except KeyboardInterrupt:
                stop_flag.stop()
            print()
            flood_thread.join(timeout=5)
            continue
        if cmd == "phase":
            plan = args[0] if args else "default"
            run_phase(cfg, plan)
            continue

        print(f"unknown command: {cmd!r} — type 'help'")

    return 0


# ── Argument parser ───────────────────────────────────────────────────────────

def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        description="RamShield extreme real-time attack simulator",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    p.add_argument("--host", default=DEFAULT_HOST)
    p.add_argument("--port", type=int, default=DEFAULT_PORT)
    p.add_argument("--workers", type=int, default=256)
    p.add_argument("--batch-size", type=int, default=2000)
    p.add_argument("--mode", choices=MODES, default="mixed")
    p.add_argument("--target-ip", default="10.255.0.1")
    p.add_argument("--read-response", action="store_true")
    p.add_argument("--no-reconnect", action="store_true")
    p.add_argument("--socket-buffer", type=int, default=1 << 20)

    sub = p.add_subparsers(dest="command", required=True)

    # burst
    b = sub.add_parser("burst", help="Send fixed event count as fast as possible")
    b.add_argument("--events", type=int, default=500_000)

    # flood
    f = sub.add_parser("flood", help="Continuous flood for duration (real-time)")
    f.add_argument("--duration", type=float, default=60.0, help="Seconds (0 = until Ctrl+C)")
    f.add_argument("--max-events", type=int, default=None, help="Optional cap")

    # phase
    ph = sub.add_parser("phase", help="Multi-phase coordinated attack")
    ph.add_argument("--plan", choices=list(PHASE_PRESETS.keys()), default="extreme")
    ph.add_argument("--modes", nargs="+", help="Custom mode sequence (overrides --plan)")

    # interactive
    sub.add_parser("interactive", help="REPL with live commands")

    # exec — non-interactive key=value
    ex = sub.add_parser("exec", help="Run one action via key=value pairs")
    ex.add_argument("action", choices=["burst", "flood", "phase", "stats", "check"])
    ex.add_argument("params", nargs="*", help="e.g. duration=60 mode=volumetric workers=512")

    # query
    st = sub.add_parser("stats", help="Query RamShield stats")
    ck = sub.add_parser("check", help="Query IP block status")
    ck.add_argument("ip")

    return p


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    cfg = cfg_from_ns(args)

    try:
        if args.command == "burst":
            stats = run_burst(cfg, args.events)
            print_results(stats, cfg)
            return 0

        if args.command == "flood":
            stop = StopFlag()
            duration = None if args.duration == 0 else args.duration
            try:
                stats = run_flood(cfg, duration=duration, max_events=args.max_events, stop=stop)
            except KeyboardInterrupt:
                stop.stop()
                stats = LiveStats()
            print_results(stats, cfg)
            return 0

        if args.command == "phase":
            run_phase(cfg, args.plan, args.modes)
            return 0

        if args.command == "interactive":
            return repl(cfg)

        if args.command == "exec":
            kv = parse_kv_args(args.params)
            apply_kv(cfg, kv)
            if args.action == "burst":
                n = int(kv.get("events", "500000"))
                stats = run_burst(cfg, n)
                print_results(stats, cfg)
            elif args.action == "flood":
                dur = float(kv.get("duration", "60"))
                stop = StopFlag()
                try:
                    stats = run_flood(cfg, duration=dur if dur > 0 else None, stop=stop)
                except KeyboardInterrupt:
                    stop.stop()
                    stats = LiveStats()
                print_results(stats, cfg)
            elif args.action == "phase":
                run_phase(cfg, kv.get("plan", "extreme"))
            elif args.action == "stats":
                cli_stats(cfg)
            elif args.action == "check":
                ip = kv.get("ip", cfg.target_ip)
                cli_check(cfg, ip)
            return 0

        if args.command == "stats":
            cli_stats(cfg)
            return 0

        if args.command == "check":
            cli_check(cfg, args.ip)
            return 0

    except ConnectionRefusedError:
        print(f"connection refused — is RamShield running on {cfg.host}:{cfg.port}?", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        print("\naborted", file=sys.stderr)
        return 130

    return 1


if __name__ == "__main__":
    raise SystemExit(main())
