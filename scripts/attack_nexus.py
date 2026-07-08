#!/usr/bin/env python3
"""
RamShield Nexus — next-gen attack simulator for authorized RamShield testing.

Inspired by multi-vector patterns documented in OWASP/Wiz/MazeBolt L3/L4/L7
taxonomies, k6 ramping, and open-source stress frameworks (mixed-mode rotation,
entropy botnets, slow/low + volumetric combos).

Maps real attack *classes* to RamShield IPC connection reports:
  ip, bytes, status_code, proto_fp

Usage:
  ./scripts/attack_nexus.py run --profile l7_http_flood --duration 60
  ./scripts/attack_nexus.py run --profile red_team_full
  ./scripts/attack_nexus.py shell
  ./scripts/attack_nexus.py profiles list

Authorized testing only — localhost / systems you own.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import random
import shlex
import socket
import sys
import threading
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

SCRIPT_DIR = Path(__file__).resolve().parent
DEFAULT_HOST = "127.0.0.1"
DEFAULT_PORT = 7890

# ── Protocol fingerprint clusters (simulated user-agents / stacks) ─────────────

PROTO_CLUSTERS: Dict[str, Tuple[int, int]] = {
    "http":       (0x1000, 0x10FF),
    "http_post":  (0x1100, 0x11FF),
    "slow":       (0x2000, 0x20FF),
    "tcp_syn":    (0x3000, 0x30FF),
    "udp":        (0x4000, 0x40FF),
    "dns":        (0x5000, 0x50FF),
    "api":        (0x6000, 0x60FF),
    "mixed_malware": (0x7000, 0x7FFF),
}

RFC5737 = [
    (198, 51, 100),
    (203, 0, 113),
    (192, 0, 2),
]


# ── Config & state ─────────────────────────────────────────────────────────────

@dataclass
class NexusConfig:
    host: str = DEFAULT_HOST
    port: int = DEFAULT_PORT
    workers: int = 128
    batch_size: int = 1500
    batch_variance: float = 0.35
    target_ip: str = "10.255.0.99"
    profile: str = "l7_http_flood"
    mode: str = "pareto_hot"
    read_response: bool = False
    reconnect: bool = True
    socket_buffer: int = 1 << 20
    seed: Optional[int] = None
    jitter_ms: Tuple[int, int] = (0, 5)
    ramp_start_eps: float = 0.0
    ramp_end_eps: float = 0.0
    ramp_seconds: float = 0.0
    status_weights: Dict[str, int] = field(default_factory=lambda: {"200": 90, "404": 5, "500": 5})
    bytes_range: Tuple[int, int] = (128, 4096)
    bytes_pareto: Optional[Tuple[float, float, float]] = None
    proto_cluster: str = "http"
    hot_ip_ratio: float = 0.15
    subnet_concentration: float = 0.3
    entropy: str = "medium"
    burst_multiplier: float = 1.0
    ipv4_private_bias: float = 0.4


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

    def snap(self) -> Tuple[int, int, int, int, float]:
        with self.lock:
            e = max(time.perf_counter() - self.started, 1e-9)
            return self.sent_events, self.sent_batches, self.errors, self.reconnects, e


class StopFlag:
    def __init__(self) -> None:
        self._ev = threading.Event()

    def stop(self) -> None:
        self._ev.set()

    def stopped(self) -> bool:
        return self._ev.is_set()

    def clear(self) -> None:
        self._ev.clear()


# ── Profile loading ────────────────────────────────────────────────────────────

def load_profiles() -> Dict[str, Any]:
    path = SCRIPT_DIR / "profiles.json"
    if not path.exists():
        return {}
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    return {k: v for k, v in data.items() if not k.startswith("_")}


def apply_profile(cfg: NexusConfig, name: str, profiles: Dict[str, Any]) -> None:
    if name not in profiles:
        raise KeyError(f"unknown profile {name!r}")
    p = profiles[name]
    if "chain" in p:
        return
    cfg.profile = name
    for key, val in p.items():
        if key in ("description", "chain", "chain_duration_sec"):
            continue
        if key == "status_weights":
            cfg.status_weights = {str(k): int(v) for k, v in val.items()}
        elif key == "bytes_pareto":
            cfg.bytes_pareto = tuple(float(x) for x in val)
            cfg.bytes_range = (int(val[0]), int(val[1]))
        elif key == "bytes_range":
            cfg.bytes_range = (int(val[0]), int(val[1]))
            cfg.bytes_pareto = None
        elif key == "jitter_ms":
            cfg.jitter_ms = (int(val[0]), int(val[1]))
        elif hasattr(cfg, key):
            setattr(cfg, key, val)


# ── Random traffic engine ──────────────────────────────────────────────────────

class TrafficEngine:
    """Stateful per-worker RNG with Zipf/Pareto samplers."""

    def __init__(self, cfg: NexusConfig, worker_id: int, master_seed: int) -> None:
        self.cfg = cfg
        self.wid = worker_id
        self.rng = random.Random(master_seed ^ (worker_id * 0x9E3779B9))
        self._botnet_pool = [self._rand_public_ip() for _ in range(500 + worker_id * 50)]
        self._zipf_pool = list(range(1, len(self._botnet_pool) + 1))
        self._scan_octet = self.rng.randint(1, 254)
        self._burst_phase = self.rng.random()

    def _rand_public_ip(self) -> str:
        while True:
            a, b, c, d = (self.rng.randint(1, 223), self.rng.randint(0, 255),
                            self.rng.randint(0, 255), self.rng.randint(1, 254))
            if a in (10, 127) or (a == 172 and 16 <= b <= 31) or (a == 192 and b == 168):
                if self.rng.random() > self.cfg.ipv4_private_bias:
                    continue
            return f"{a}.{b}.{c}.{d}"

    def _pick_subnet_ip(self) -> str:
        p = self.rng.choice(RFC5737)
        return f"{p[0]}.{p[1]}.{p[2]}.{self.rng.randint(1, 254)}"

    def _pareto_hot_ip(self) -> str:
        c = self.cfg
        if self.rng.random() < c.hot_ip_ratio:
            return c.target_ip
        if self.rng.random() < c.subnet_concentration:
            return self._pick_subnet_ip()
        return self._rand_public_ip()

    def _botnet_ip(self) -> str:
        c = self.cfg
        if c.entropy == "max":
            return self._rand_public_ip()
        if c.entropy == "high":
            return self.rng.choice(self._botnet_pool)
        idx = min(int(self.rng.paretovariate(1.2) * 10), len(self._botnet_pool) - 1)
        return self._botnet_pool[idx]

    def _zipf_ip(self) -> str:
        rank = self.rng.randint(1, min(200, len(self._botnet_pool)))
        return self._botnet_pool[rank - 1]

    def _scan_ip(self) -> str:
        self._scan_octet = (self._scan_octet + self.rng.randint(1, 7)) % 254 + 1
        base = self.rng.choice(RFC5737)
        return f"{base[0]}.{base[1]}.{self._scan_octet}.{self.rng.randint(1, 254)}"

    def _synwave_ip(self) -> str:
        if self.rng.random() < 0.35:
            return self.cfg.target_ip
        return f"10.{self.wid % 256}.{self.rng.randint(0, 255)}.{self.rng.randint(1, 254)}"

    def pick_ip(self) -> str:
        m = self.cfg.mode
        if m == "volumetric":
            return self.cfg.target_ip
        if m == "subnet":
            return self._pick_subnet_ip()
        if m == "multi_subnet":
            p = self.rng.choice(RFC5737 + [(100, 64, 0), (203, 0, 113)])
            return f"{p[0]}.{p[1]}.{p[2]}.{self.rng.randint(1, 254)}"
        if m == "botnet":
            return self._botnet_ip()
        if m == "synwave":
            return self._synwave_ip()
        if m == "scan_rotation":
            return self._scan_ip()
        if m == "pareto_hot":
            return self._pareto_hot_ip()
        if m == "zipf_asn":
            return self._zipf_ip()
        return self._pareto_hot_ip()

    def pick_status(self) -> int:
        weights = self.cfg.status_weights
        codes = [int(k) for k in weights]
        w = [weights[str(c)] for c in codes]
        return self.rng.choices(codes, weights=w, k=1)[0]

    def pick_bytes(self) -> int:
        c = self.cfg
        if c.bytes_pareto:
            lo, hi, alpha = c.bytes_pareto
            u = self.rng.random()
            x = lo / (u ** (1.0 / alpha))
            return int(min(max(x, lo), hi))
        lo, hi = c.bytes_range
        if self.rng.random() < 0.1:
            return self.rng.randint(lo, hi)
        mid = (lo + hi) // 2
        spread = (hi - lo) // 4
        return int(self.rng.gauss(mid, max(spread, 1)))

    def pick_proto(self) -> int:
        lo, hi = PROTO_CLUSTERS.get(self.cfg.proto_cluster, (0, 0xFFFF))
        base = self.rng.randint(lo, hi)
        if self.rng.random() < 0.2:
            base ^= self.rng.randint(0, 0xFF)
        return base & 0xFFFF

    def make_event(self) -> dict:
        burst = self.cfg.burst_multiplier
        if burst > 1.0 and self.rng.random() < 0.05 * burst:
            bytes_in = self.rng.randint(self.cfg.bytes_range[1], self.cfg.bytes_range[1] * 2)
        else:
            bytes_in = max(1, self.pick_bytes())
        return {
            "ip": self.pick_ip(),
            "bytes": bytes_in,
            "status_code": self.pick_status(),
            "proto_fp": self.pick_proto(),
        }

    def batch_size(self) -> int:
        base = self.cfg.batch_size
        v = self.cfg.batch_variance
        delta = int(base * v)
        return max(1, self.rng.randint(base - delta, base + delta))

    def sleep_jitter(self) -> None:
        lo, hi = self.cfg.jitter_ms
        if hi <= 0:
            return
        time.sleep(self.rng.uniform(lo, hi) / 1000.0)


def master_seed(cfg: NexusConfig) -> int:
    if cfg.seed is not None:
        return cfg.seed
    return random.randrange(1 << 62)


# ── IPC I/O ───────────────────────────────────────────────────────────────────

def tune_socket(sock: socket.socket, buf: int) -> None:
    sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    try:
        sock.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, buf)
    except OSError:
        pass


def send_batch(sock: socket.socket, events: List[dict], read_resp: bool) -> None:
    line = json.dumps({"type": "report_connections", "events": events}, separators=(",", ":")) + "\n"
    sock.sendall(line.encode())
    if read_resp:
        sock.settimeout(1.0)
        sock.recv(4096)


def ipc_json(host: str, port: int, payload: dict) -> dict:
    with socket.create_connection((host, port), timeout=5) as s:
        tune_socket(s, 65536)
        s.sendall((json.dumps(payload) + "\n").encode())
        s.settimeout(3)
        return json.loads(s.recv(65536).decode())


# ── Ramp controller (k6-style) ─────────────────────────────────────────────────

class RampController:
    def __init__(self, start_eps: float, end_eps: float, seconds: float) -> None:
        self.start = start_eps
        self.end = end_eps
        self.seconds = seconds
        self.t0 = time.perf_counter()

    def throttle_batch(self, default_batch: int) -> int:
        if self.seconds <= 0:
            return default_batch
        t = time.perf_counter() - self.t0
        if t >= self.seconds:
            target_eps = self.end
        else:
            frac = t / self.seconds
            target_eps = self.start + (self.end - self.start) * frac
        if target_eps <= 0:
            return default_batch
        scale = target_eps / max(self.end, self.start, 1.0)
        return max(1, int(default_batch * scale))


# ── Workers ────────────────────────────────────────────────────────────────────

def flood_worker(
    wid: int,
    cfg: NexusConfig,
    stop: StopFlag,
    stats: LiveStats,
    seed: int,
    duration: Optional[float],
    max_events: Optional[int],
    ramp: Optional[RampController],
) -> None:
    eng = TrafficEngine(cfg, wid, seed)
    deadline = time.perf_counter() + duration if duration else None
    sent = 0
    sock: Optional[socket.socket] = None

    while not stop.stopped():
        if deadline and time.perf_counter() >= deadline:
            break
        if max_events is not None and sent >= max_events:
            break

        n = eng.batch_size()
        if ramp:
            n = ramp.throttle_batch(n)
        if max_events is not None:
            n = min(n, max_events - sent)

        try:
            if sock is None:
                sock = socket.create_connection((cfg.host, cfg.port), timeout=10)
                tune_socket(sock, cfg.socket_buffer)
            batch = [eng.make_event() for _ in range(n)]
            send_batch(sock, batch, cfg.read_response)
            stats.add(events=len(batch), batches=1)
            sent += len(batch)
            eng.sleep_jitter()
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
                time.sleep(eng.rng.uniform(0.001, 0.05))
            else:
                break

    if sock:
        try:
            sock.close()
        except OSError:
            pass


def run_flood(
    cfg: NexusConfig,
    *,
    duration: Optional[float] = None,
    max_events: Optional[int] = None,
    stop: Optional[StopFlag] = None,
    stats: Optional[LiveStats] = None,
) -> LiveStats:
    stop = stop or StopFlag()
    stats = stats or LiveStats()
    stats.started = time.perf_counter()
    seed = master_seed(cfg)
    ramp = None
    if cfg.ramp_seconds > 0 and cfg.ramp_end_eps > 0:
        ramp = RampController(cfg.ramp_start_eps, cfg.ramp_end_eps, cfg.ramp_seconds)

    threads = [
        threading.Thread(
            target=flood_worker,
            args=(i, cfg, stop, stats, seed, duration, max_events, ramp),
            daemon=True,
            name=f"nexus-{i}",
        )
        for i in range(cfg.workers)
    ]
    for t in threads:
        t.start()

    try:
        while any(t.is_alive() for t in threads):
            ev, bt, er, rc, el = stats.snap()
            sys.stdout.write(
                f"\r[NEXUS] {cfg.profile} eps={ev/el:,.0f} events={ev:,} err={er} rc={rc}   "
            )
            sys.stdout.flush()
            if duration and el >= duration + 1:
                break
            time.sleep(0.25)
    except KeyboardInterrupt:
        stop.stop()
    print()
    stop.stop()
    for t in threads:
        t.join(timeout=5)
    return stats


def run_chain(cfg: NexusConfig, profiles: Dict[str, Any], name: str) -> None:
    p = profiles[name]
    chain = p.get("chain", [])
    dur = float(p.get("chain_duration_sec", 30))
    print(f"=== CHAIN {name}: {len(chain)} phases × {dur}s ===")
    for i, prof in enumerate(chain, 1):
        print(f"\n--- phase {i}/{len(chain)}: {prof} ---")
        apply_profile(cfg, prof, profiles)
        run_flood(cfg, duration=dur)


def run_burst(cfg: NexusConfig, events: int) -> LiveStats:
    return run_flood(cfg, max_events=events)


# ── Interactive shell ─────────────────────────────────────────────────────────

SHELL_HELP = """
Commands:
  help                          This help
  show                          Print all settings
  profiles [list|load NAME]     List or load attack profile
  set KEY VALUE                 Any config key (workers, batch_size, profile, …)
  set status_weights JSON       e.g. {"200":80,"404":20}
  set jitter LO HI              Milliseconds between batches
  set bytes LO HI               Fixed byte range
  set pareto LO HI ALPHA        Pareto byte distribution
  seed [N|random]               RNG seed
  ramp START END SECONDS        k6-style EPS ramp (approximate)
  flood [SEC]                   Flood SEC seconds (default 60)
  burst N                       Send N total events
  chain NAME                    Run profile chain (e.g. red_team_full)
  stop                          Stop active flood
  stats | check IP              Query RamShield
  export FILE                   Save session JSON
  import FILE                   Load session JSON
  quit
"""


def shell(cfg: NexusConfig, profiles: Dict[str, Any]) -> int:
    stop = StopFlag()
    stats = LiveStats()
    flood_thread: Optional[threading.Thread] = None
    macros: Dict[str, Dict[str, Any]] = {}

    print("=== RamShield NEXUS shell (authorized testing) ===")
    print("Type 'help'. Profiles:", ", ".join(sorted(profiles.keys())[:8]), "…")

    def start_flood(dur: float) -> None:
        nonlocal flood_thread
        stop.clear()
        local_stop = StopFlag()

        def _run() -> None:
            run_flood(cfg, duration=dur, stop=local_stop, stats=stats)

        flood_thread = threading.Thread(target=_run, daemon=True)
        flood_thread.start()

    while True:
        try:
            line = input("nexus> ").strip()
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
            print(SHELL_HELP)
            continue
        if cmd == "show":
            print(json.dumps(asdict(cfg), indent=2, default=str))
            continue
        if cmd == "profiles":
            if not args or args[0] == "list":
                for n, p in profiles.items():
                    desc = p.get("description", "")[:70]
                    print(f"  {n:22} {desc}")
            elif args[0] == "load" and len(args) > 1:
                try:
                    apply_profile(cfg, args[1], profiles)
                    print(f"loaded profile {args[1]!r}")
                except KeyError as e:
                    print(e)
            continue
        if cmd == "set" and len(args) >= 2:
            key, val = args[0], " ".join(args[1:])
            try:
                if key == "status_weights":
                    cfg.status_weights = {str(k): int(v) for k, v in json.loads(val).items()}
                elif key == "jitter":
                    a, b = val.split()
                    cfg.jitter_ms = (int(a), int(b))
                elif key == "bytes":
                    a, b = val.split()
                    cfg.bytes_range = (int(a), int(b))
                    cfg.bytes_pareto = None
                elif key == "pareto":
                    a, b, c = val.split()
                    cfg.bytes_pareto = (float(a), float(b), float(c))
                elif key == "workers":
                    cfg.workers = max(1, int(val))
                elif key == "batch_size":
                    cfg.batch_size = max(1, int(val))
                elif key == "batch_variance":
                    cfg.batch_variance = float(val)
                elif key == "hot_ip_ratio":
                    cfg.hot_ip_ratio = float(val)
                elif key == "subnet_concentration":
                    cfg.subnet_concentration = float(val)
                elif key == "burst_multiplier":
                    cfg.burst_multiplier = float(val)
                elif key == "entropy":
                    cfg.entropy = val
                elif key == "mode":
                    cfg.mode = val
                elif key == "target":
                    cfg.target_ip = val
                elif key == "host":
                    cfg.host = val
                elif key == "port":
                    cfg.port = int(val)
                elif key == "profile":
                    apply_profile(cfg, val, profiles)
                else:
                    print(f"unknown key {key!r}")
                    continue
                print(f"ok {key}={getattr(cfg, key, val)}")
            except Exception as e:
                print(f"set failed: {e}")
            continue
        if cmd == "seed":
            if not args or args[0] == "random":
                cfg.seed = None
            else:
                cfg.seed = int(args[0])
            print(f"seed={cfg.seed}")
            continue
        if cmd == "ramp" and len(args) == 3:
            cfg.ramp_start_eps, cfg.ramp_end_eps, cfg.ramp_seconds = float(args[0]), float(args[1]), float(args[2])
            print(f"ramp {cfg.ramp_start_eps}→{cfg.ramp_end_eps} over {cfg.ramp_seconds}s")
            continue
        if cmd == "macro" and len(args) >= 2:
            macros[args[0]] = asdict(cfg)
            print(f"saved macro {args[0]!r}")
            continue
        if cmd == "macro" and len(args) == 1 and args[0] in macros:
            for k, v in macros[args[0]].items():
                if hasattr(cfg, k):
                    setattr(cfg, k, v)
            print(f"restored macro {args[0]!r}")
            continue
        if cmd == "flood":
            dur = float(args[0]) if args else 60.0
            stats = LiveStats()
            start_flood(dur)
            print(f"flood {dur}s profile={cfg.profile} workers={cfg.workers}")
            continue
        if cmd == "burst" and args:
            stats = LiveStats()
            run_burst(cfg, int(args[0]))
            ev, _, _, _, el = stats.snap()
            print(f"burst done: {ev:,} events in {el:.1f}s")
            continue
        if cmd == "chain" and args:
            run_chain(cfg, profiles, args[0])
            continue
        if cmd == "stop":
            stop.stop()
            print("stopped")
            continue
        if cmd == "stats":
            print(json.dumps(ipc_json(cfg.host, cfg.port, {"type": "get_stats"}), indent=2))
            continue
        if cmd == "check" and args:
            print(json.dumps(ipc_json(cfg.host, cfg.port, {"type": "check_ip", "ip": args[0]}), indent=2))
            continue
        if cmd == "export" and args:
            with open(args[0], "w", encoding="utf-8") as f:
                json.dump(asdict(cfg), f, indent=2, default=str)
            print(f"wrote {args[0]}")
            continue
        if cmd == "import" and args:
            with open(args[0], encoding="utf-8") as f:
                data = json.load(f)
            for k, v in data.items():
                if hasattr(cfg, k):
                    setattr(cfg, k, v)
            print(f"loaded {args[0]}")
            continue

        print(f"unknown: {cmd!r} — type help")

    return 0


# ── CLI ────────────────────────────────────────────────────────────────────────

def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description="RamShield Nexus attack simulator")
    p.add_argument("--host", default=DEFAULT_HOST)
    p.add_argument("--port", type=int, default=DEFAULT_PORT)
    p.add_argument("--workers", type=int, default=128)
    p.add_argument("--batch-size", type=int, default=1500)
    p.add_argument("--target", default="10.255.0.99")
    p.add_argument("--seed", type=int, default=None)

    sub = p.add_subparsers(dest="cmd", required=True)

    r = sub.add_parser("run", help="Run single profile")
    r.add_argument("--profile", default="l7_http_flood")
    r.add_argument("--duration", type=float, default=60.0)
    r.add_argument("--events", type=int, default=None)
    r.add_argument("--ramp", nargs=3, metavar=("START", "END", "SEC"), type=float)

    sub.add_parser("shell", help="Interactive customizable shell")

    pl = sub.add_parser("profiles", help="List profiles")
    pl.add_argument("action", nargs="?", default="list")
    pl.add_argument("name", nargs="?", default=None)

    return p


def main() -> int:
    profiles = load_profiles()
    args = build_parser().parse_args()
    cfg = NexusConfig(
        host=args.host,
        port=args.port,
        workers=args.workers,
        batch_size=args.batch_size,
        target_ip=args.target,
        seed=args.seed,
    )

    try:
        if args.cmd == "profiles":
            if args.action == "list" or not args.name:
                for n, p in profiles.items():
                    print(f"{n:22} {p.get('description', '')}")
            else:
                print(json.dumps(profiles.get(args.name, {}), indent=2))
            return 0

        if args.cmd == "shell":
            if cfg.profile in profiles:
                apply_profile(cfg, cfg.profile, profiles)
            return shell(cfg, profiles)

        if args.cmd == "run":
            if args.profile in profiles:
                p = profiles[args.profile]
                if "chain" in p:
                    run_chain(cfg, profiles, args.profile)
                    return 0
                apply_profile(cfg, args.profile, profiles)
            else:
                cfg.profile = args.profile
            if args.ramp:
                cfg.ramp_start_eps, cfg.ramp_end_eps, cfg.ramp_seconds = args.ramp
            stats = run_flood(
                cfg,
                duration=args.duration if not args.events else None,
                max_events=args.events,
            )
            ev, _, er, _, el = stats.snap()
            print(f"done: {ev:,} events, {ev/el:,.0f} eps, {er} errors")
            return 0

    except ConnectionRefusedError:
        print(f"cannot connect to {cfg.host}:{cfg.port} — start RamShield first", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        print("\naborted", file=sys.stderr)
        return 130

    return 1


if __name__ == "__main__":
    raise SystemExit(main())
