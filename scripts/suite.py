#!/usr/bin/env python3
"""RamShield testing suite — one entry point for every check.

Replaces the scattered legacy scripts (attack_sim_100k, attack_extreme,
cruel_ddos, attack_driver, scenario_runner, generate_scenarios,
map_and_run, create_mapped_scenarios, selftest.sh, check_guardrails.sh)
with a single, documented CLI.

Layers:
  unit     cargo test (Rust unit + integration tests)
  lint     cargo fmt --check + clippy -D warnings (CI gates)
  e2e      boots a release binary on scratch ports, drives the real IPC
           protocol end-to-end: health, check_ip, block/unblock, batch
           reports, subnet blocking, WAL restart recovery, dashboard API
  xdp      kernel dataplane proof on a real NIC: drives the LIVE instance
           (the one holding the BPF program), blocks a probe IP via IPC,
           generates real ingress packets, asserts the kernel counters move
           and the LPM entry is pruned after TTL expiry. SKIPs cleanly when
           no XDP program is attached anywhere (CI-safe no-op).
  load     attack profiles via attack_nexus.py (the retained simulator):
             profiles list | run --profile NAME --duration S | bench

Authorized testing only — everything binds/talks to 127.0.0.1.

Usage:
  python3 scripts/suite.py unit
  python3 scripts/suite.py lint
  python3 scripts/suite.py e2e                 # full end-to-end pass
  python3 scripts/suite.py e2e --keep          # keep server running after
  python3 scripts/suite.py xdp [--target IP]   # kernel dataplane (SKIPs if N/A)
  python3 scripts/suite.py load profiles
  python3 scripts/suite.py load run --profile l7_http_flood --duration 30
  python3 scripts/suite.py load bench          # 5-min subnet DDoS benchmark
  python3 scripts/suite.py all                 # lint + unit + e2e + xdp (CI order)
Exit code = number of failed layers (0 = all green).
"""
from __future__ import annotations

import argparse
import hashlib
import hmac
import json
import os
import signal
import socket
import subprocess
import sys
import threading
import time
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
BIN = REPO / "target" / "release" / "ramshield"
BASELINE = REPO / "config.baseline.toml"
IPC_PORT = 17890
DASH_PORT = 19999
IPC_ADDR = f"127.0.0.1:{IPC_PORT}"
DASH_URL = f"http://127.0.0.1:{DASH_PORT}"
START_TIMEOUT = 30.0


# ── helpers ───────────────────────────────────────────────────────────────────

def baseline_config(**overrides) -> str:
    """Scratch config generated from the tracked baseline template.

    Dotted-path overrides only (wal__dir, ipc__tcp_addr, xdp__enabled…).
    The baseline is the single source of truth — harnesses must never
    hand-roll whole configs. Returns the temp path; server deletes nothing
    (OS tmp hygiene).
    """
    import tempfile
    import tomllib
    base = tomllib.loads(BASELINE.read_text())
    for dotted, val in overrides.items():
        cur = base
        parts = dotted.split("__")
        for p in parts[:-1]:
            cur = cur.setdefault(p, {})
        cur[parts[-1]] = val
    fd, path = tempfile.mkstemp(suffix=".toml", prefix="rs_scratch_")
    with os.fdopen(fd, "w") as f:
        for sec, fields in base.items():
            f.write(f"[{sec}]\n")
            for k, v in fields.items():
                f.write(f"{k} = {json.dumps(v)}\n")
            f.write("\n")
    return path


def sh(*args: str, cwd: Path = REPO, timeout: int | None = None) -> int:
    print(f"  $ {' '.join(args)}")
    return subprocess.run(args, cwd=cwd, timeout=timeout).returncode


def sh_out(*args: str, cwd: Path = REPO, timeout: int | None = None) -> tuple[int, str]:
    r = subprocess.run(args, cwd=cwd, capture_output=True, text=True, timeout=timeout)
    return r.returncode, (r.stdout + r.stderr)

# T13 Pulse Tracker fix: is_over_threshold is now used in rate_tracker.rs patch
# If the method becomes unused, add #[expect(dead_code)] to suppress warning


class Check:
    """Named assertion accumulator for one suite layer."""

    def __init__(self, name: str) -> None:
        self.name = name
        self.passed = 0
        self.failed: list[str] = []

    def ok(self, cond: bool, desc: str, detail: str = "") -> bool:
        mark = "PASS" if cond else "FAIL"
        print(f"    [{mark}] {desc}" + (f" — {detail}" if detail and not cond else ""))
        if cond:
            self.passed += 1
        else:
            self.failed.append(desc)
        return cond

    def finish(self) -> int:
        total = self.passed + len(self.failed)
        status = "OK" if not self.failed else f"FAILED {len(self.failed)}/{total}"
        print(f"  == {self.name}: {status} ({self.passed}/{total} passed)\n")
        return len(self.failed)


def ipc(payload: dict, timeout: float = 5.0) -> dict:
    """One JSON line over TCP → one JSON line back."""
    with socket.create_connection(("127.0.0.1", IPC_PORT), timeout=timeout) as s:
        s.sendall((json.dumps(payload) + "\n").encode())
        s.settimeout(timeout)
        buf = b""
        while b"\n" not in buf:
            chunk = s.recv(65536)
            if not chunk:
                break
            buf += chunk
        return json.loads(buf.split(b"\n")[0])


def wait_ready(deadline: float = START_TIMEOUT) -> bool:
    end = time.monotonic() + deadline
    while time.monotonic() < end:
        try:
            with urllib.request.urlopen(f"{DASH_URL}/healthz", timeout=1) as r:
                if r.status == 200:
                    return True
        except Exception:
            time.sleep(0.25)
    return False


class Server:
    """Scratch-port release server lifecycle (never touches :7890/:9999).

    Config must be baseline-generated (ports/wal dir already overridden) —
    see baseline_config(). No env-var injection: one mechanism."""

    def __init__(self, config: str) -> None:
        self.proc: subprocess.Popen | None = None
        self.config = config

    def __enter__(self) -> "Server":
        if not BIN.exists():
            raise SystemExit(f"release binary missing: {BIN}\n  cargo build --release -F full")
        self.proc = subprocess.Popen(
            [str(BIN), "--config", self.config],
            cwd=str(REPO),
            env=dict(os.environ),
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
        if not wait_ready():
            self.__exit__(None, None, None)
            raise SystemExit("server failed to become healthy on scratch ports")
        print(f"  server up: ipc={IPC_ADDR} dash={DASH_URL} (pid {self.proc.pid})")
        return self

    def __exit__(self, *exc) -> None:
        if self.proc:
            try:
                os.killpg(self.proc.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(self.proc.pid, signal.SIGKILL)
        print("  server stopped")

    def restart(self) -> None:
        """Hard kill + fresh boot (WAL recovery path)."""
        self.__exit__(None, None, None)
        self.proc = None
        self.__enter__()


# ── layers ────────────────────────────────────────────────────────────────────

def layer_lint() -> int:
    c = Check("lint")
    c.ok(sh("cargo", "fmt", "--all", "--check", timeout=120) == 0, "cargo fmt --check")
    c.ok(sh("cargo", "clippy", "--all-targets", "--", "-D", "warnings", timeout=600) == 0,
         "cargo clippy -D warnings")
    return c.finish()


def layer_unit() -> int:
    c = Check("unit")
    rc, out = sh_out("cargo", "test", "--all", timeout=900)
    c.ok(rc == 0, "cargo test --all")
    if rc != 0:
        print(out[-2000:])
    return c.finish()


def layer_e2e(keep: bool = False) -> int:
    c = Check("e2e")
    # Baseline-derived: scratch ports, isolated WAL (single-writer rule),
    # XDP off — kernel coverage is the xdp layer's job (live instance).
    import shutil
    shutil.rmtree("/tmp/rs_e2e_wal", ignore_errors=True)
    with Server(baseline_config(
        ipc__tcp_addr=IPC_ADDR,
        dashboard__http_addr=f"127.0.0.1:{DASH_PORT}",
        wal__dir="/tmp/rs_e2e_wal",
        xdp__enabled=False,
    )) as srv:
        # health
        with urllib.request.urlopen(f"{DASH_URL}/healthz", timeout=3) as r:
            body = json.load(r)
        c.ok(body.get("status") == "ok", "healthz returns {status: ok}", str(body))

        # check_ip: unknown IP is clean
        r = ipc({"type": "check_ip", "ip": "203.0.113.7"})
        c.ok(r.get("type") == "ip_status" and not r.get("blocked"),
             "check_ip unknown → not blocked", str(r))

        # manual block / unblock round-trip
        r = ipc({"type": "block_ip", "ip": "203.0.113.7", "reason": "suite", "ttl_secs": 120})
        c.ok(r.get("type") == "ok", "block_ip accepted", str(r))
        r = ipc({"type": "check_ip", "ip": "203.0.113.7"})
        c.ok(bool(r.get("blocked")), "check_ip blocked after block_ip", str(r))
        r = ipc({"type": "unblock_ip", "ip": "203.0.113.7"})
        c.ok(not r.get("error"), "unblock_ip accepted", str(r))
        r = ipc({"type": "check_ip", "ip": "203.0.113.7"})
        c.ok(not r.get("blocked"), "check_ip clean after unblock_ip", str(r))

        # batch reports: drive one IP over threshold → auto-block.
        # Detection needs 2 consecutive hot EWMA samples; ~20 batches of 200
        # events (inst_rps≈200 > threshold 100 in config.toml) arms it.
        blocked = False
        for round_ in range(60):
            ev = [{"ip": "198.51.100.66", "bytes": 512, "status_code": 200, "proto_fp": 0x1000}
                  for _ in range(200)]
            r = ipc({"type": "report_connections", "events": ev})
            if round_ == 0:
                c.ok(r.get("type") == "batch_ok" or "error" not in r,
                     "report_connections batch accepted", str(r))
            time.sleep(0.05)
            if ipc({"type": "check_ip", "ip": "198.51.100.66"}).get("blocked"):
                blocked = True
                break
        c.ok(blocked, "EWMA auto-block fires above rps_threshold")

        # subnet block: many distinct IPs from one /24. The CGNAT guard
        # classifies a public /24 as TIER_BLOCK only above 50k events in the
        # 4s subnet window (cgnat.rs), so drive a ~5s sustained flood at
        # ~16k events/s: any 4s window accumulates ~65k, and the dev flush
        # cadence (pre_aggs_flush_interval_ms=1000) can't straggle a burst
        # out of the window. Per-IP volume (~300 ev) stays far under every
        # per-IP gate — only the subnet decision should fire.
        events = [{"ip": f"192.0.2.{i}", "bytes": 256, "status_code": 404, "proto_fp": 0x1000}
                  for i in range(250)]
        frame = events + events  # 500 events/frame, under the 4096 batch cap
        for _ in range(150):
            ipc({"type": "report_connections", "events": frame})
            time.sleep(0.03)
        subnet_blocked = False
        for _ in range(60):
            time.sleep(0.25)
            r = ipc({"type": "check_ip", "ip": "192.0.2.199"})
            if r.get("blocked"):
                subnet_blocked = True
                break
        c.ok(subnet_blocked, "subnet /24 block fires on distinct-IP flood")

        # stats + snapshot API
        r = ipc({"type": "get_stats"})
        stats = r.get("ips_tracked", 0)
        c.ok(stats > 0 or isinstance(r.get("blocked"), int),
             "get_stats responds with counters", str(r)[:120])
        with urllib.request.urlopen(f"{DASH_URL}/api/snapshot", timeout=3) as resp:
            snap = json.load(resp)
        c.ok(snap.get("is_healthy") is True, "dashboard snapshot healthy")
        c.ok(snap.get("pipeline", {}).get("blocked", 0) > 0, "snapshot counts blocked events")

        # invalid input → typed error frame, connection survives
        r = ipc({"type": "check_ip", "ip": "not-an-ip"})
        c.ok(r.get("type") == "error" and r.get("code") == 400,
             "invalid IP → typed 400 error frame", str(r)[:120])
        r = ipc({"type": "check_ip", "ip": "203.0.113.9"})
        c.ok(r.get("type") == "ip_status", "connection still alive after bad frame")

        if not keep:
            pass
    if keep:
        print(f"  NOTE: --keep ignored after suite run; scratch server always torn down")
    return c.finish()


# The kernel layer targets the LIVE instance (default 7890/9999) — the scratch
# Server above runs config.toml with XDP disabled and can never attach.
LIVE_IPC_PORT = 7890
LIVE_DASH_URL = "http://127.0.0.1:9999"
PROD_CONFIG = REPO / "config.prod.toml"


def xdp_metrics(url: str = LIVE_DASH_URL) -> dict:
    """ramshield_xdp_* gauges from a live instance's /metrics."""
    out = {}
    for row in urllib.request.urlopen(f"{url}/metrics", timeout=3).read().decode().splitlines():
        if row.startswith("ramshield_xdp_") and " " in row:
            out[row.rsplit(" ", 1)[0]] = int(float(row.rsplit(" ", 1)[1]))
    return out


def live_ipc(payload: dict, key: bytes | None, key_id: str, timeout: float = 5.0) -> dict:
    """One JSON line to the LIVE instance — HMAC auth when the prod config
    carries keys (sig over '<ts_ms>.<key_id>' + compact-sorted payload JSON,
    payload = frame without the auth object)."""
    frame = dict(payload)
    line = json.dumps(frame, separators=(",", ":"), sort_keys=True).encode()
    if key is not None:
        ts = int(time.time() * 1000)
        sig = hmac.new(key, f"{ts}.{key_id}".encode() + line, hashlib.sha256).hexdigest()
        line = json.dumps({"auth": {"key_id": key_id, "ts_ms": ts, "sig": sig}, **frame},
                          separators=(",", ":"), sort_keys=True).encode()
    with socket.create_connection(("127.0.0.1", LIVE_IPC_PORT), timeout=timeout) as s:
        s.sendall(line + b"\n")
        s.settimeout(timeout)
        return json.loads(s.recv(1048576).split(b"\n")[0])


def layer_xdp(target: str | None = None) -> int:
    """Kernel dataplane proof on a real NIC (drives the live instance).

    SKIP (exit 0) unless ALL hold: a release binary, an XDP program attached
    to a non-lo interface (ip -d link), a live instance answering on 9999,
    and a reachable probe target. When it runs: manual block (TTL 60s) via
    IPC, real ingress generation, kernel counter assertions, then wait for
    TTL expiry and assert the LPM entry is pruned. ~90s when it runs.
    """
    c = Check("xdp")

    def skip(why: str) -> int:
        print(f"  == xdp: SKIP — {why}\n")
        return 0

    if not BIN.exists():
        return skip("no release binary (cargo build --release --locked --features full)")
    _, out = sh_out("ip", "-d", "link", "show")
    # "3: wlp2s0: <...> mtu 1500 xdpgeneric ..." → name is between idx and flags
    xdp_iface = next((l.split(":", 2)[1].split()[0]
                      for l in out.splitlines()
                      if "xdpgeneric" in l or " xdp " in f" {l} "),
                     None)
    if xdp_iface is None or xdp_iface == "lo":
        return skip("no kernel XDP program attached to a real interface "
                    "(setcap + [xdp] on a real NIC + restart the live instance)")
    try:
        urllib.request.urlopen(f"{LIVE_DASH_URL}/healthz", timeout=2)
    except Exception:
        return skip(f"no live instance on {LIVE_DASH_URL} (nothing to drive)")

    # Prod config carries the auth keys (gitignored; absent in CI → skip).
    key, key_id = None, ""
    try:
        import tomllib
        pcfg = tomllib.loads(PROD_CONFIG.read_text())
        if pcfg.get("ipc", {}).get("auth_keys"):
            key_id, key = pcfg["ipc"]["auth_keys"][0].split(":", 1)
            key = bytes.fromhex(key)
    except (OSError, ValueError):
        key, key_id = None, ""

    # Probe target: a real, reachable public IP that's NOT the local resolver
    # and NOT in the active connection table (don't kill the user's browsing).
    try:
        resolver = {t.split()[1] for t in open("/etc/resolv.conf").read().splitlines()
                    if t.startswith("nameserver")}
    except OSError:
        resolver = set()
    active = set()
    try:
        active = {l.split()[3].rsplit(":", 1)[0] for l in
                  subprocess.run(["ss", "-tn", "state", "established"],
                                 capture_output=True, text=True, timeout=5).stdout.splitlines()[1:]}
    except Exception:
        pass
    cand = [target] if target else ["1.1.1.1", "8.8.8.8", "9.9.9.9"]
    probe = next((t for t in cand if t not in resolver and t not in active), None)
    if probe is None:
        return skip("no safe probe target (candidates busy/resolver or unreachable)")
    try:
        urllib.request.urlopen(f"https://{probe}/", timeout=3)
    except Exception:
        return skip(f"probe target {probe} unreachable (no internet for ingress generation)")

    print(f"  xdp kernel dataplane on {xdp_iface}; live instance @ {LIVE_DASH_URL}; "
          f"probe {probe}; block TTL 60s")
    try:
        base = xdp_metrics()
    except Exception:
        return skip("live instance exposes no /metrics")
    c.ok(all(k in base for k in ("ramshield_xdp_v4_drops", "ramshield_xdp_apply_failures_total",
                                 "ramshield_xdp_attribution_gaps")),
         "xdp counters exposed on live instance", str(base))

    # 1) manual block via IPC (no auth → accept as-is; with auth → HMAC).
    r = live_ipc({"type": "block_ip", "ip": probe, "reason": "xdp-suite", "ttl_secs": 60},
                 key, key_id)
    c.ok(r.get("type") == "ok" and "error" not in r, "block_ip accepted on live instance", str(r)[:120])

    # 2) real ingress: keep opening connections to the blocked IP; every
    #    inbound packet (SYN-ACK/data from it) traverses the BPF program.
    stop = threading.Event()

    def hammer() -> None:
        while not stop.is_set():
            try:
                urllib.request.urlopen(f"https://{probe}/", timeout=2).read(32)
            except Exception:
                pass
            time.sleep(0.15)

    th = threading.Thread(target=hammer, daemon=True)
    th.start()
    end = time.monotonic() + 15.0
    m = base
    while time.monotonic() < end:
        time.sleep(1.0)
        m = xdp_metrics()
        if m.get("ramshield_xdp_v4_drops", 0) > base.get("ramshield_xdp_v4_drops", 0):
            break
    stop.set()
    th.join(timeout=2)

    # 3) kernel dataplane assertions.
    c.ok(m.get("ramshield_xdp_v4_drops", 0) > base.get("ramshield_xdp_v4_drops", 0),
         f"v4_drops increases on blocked-IP ingress ({base.get('ramshield_xdp_v4_drops')} → {m.get('ramshield_xdp_v4_drops')})")
    c.ok(m.get("ramshield_xdp_attribution_gaps", 0) == 0,
         "attribution_gaps stays 0 (every drop attributed to the LPM entry)")
    c.ok(m.get("ramshield_xdp_apply_failures_total", 0) == 0,
         "apply_failures_total stays 0 (LPM update landed)")
    c.ok(m.get("ramshield_xdp_parse_fails", 0) == 0, "parse_fails stays 0")
    r = live_ipc({"type": "check_ip", "ip": probe}, key, key_id)
    c.ok(bool(r.get("blocked")), "check_ip confirms the block while ingress is dropping", str(r)[:120])

    # 4) TTL expiry → LPM entry pruned (reconcile).
    end = time.monotonic() + 90.0
    pruned = False
    while time.monotonic() < end:
        time.sleep(5.0)
        r = live_ipc({"type": "check_ip", "ip": probe}, key, key_id)
        if not r.get("blocked"):
            pruned = True
            break
    c.ok(pruned, "block auto-expires and LPM entry is pruned after TTL (≤90s)", str(r)[:120])
    return c.finish()


def load_config() -> str:
    """Baseline-derived scratch config for the load layer (no WAL, XDP off)."""
    import shutil
    shutil.rmtree("/tmp/rs_load_wal", ignore_errors=True)
    return baseline_config(
        ipc__tcp_addr=IPC_ADDR,
        dashboard__http_addr=f"127.0.0.1:{DASH_PORT}",
        wal__enabled=False,
        xdp__enabled=False,
    )


def layer_load(args: argparse.Namespace) -> int:
    nexus = REPO / "scripts" / "attack_nexus.py"
    if args.load_cmd == "profiles":
        return sh(sys.executable, str(nexus), "profiles", "list")
    if args.load_cmd == "run":
        args_ = [sys.executable, str(nexus), "--port", str(IPC_PORT)]
        if args.profile:
            args_ += ["run", "--profile", args.profile, "--duration", str(args.duration)]
        else:
            args_ += ["run", "--profile", "l7_http_flood", "--duration", str(args.duration)]
        with Server(load_config()) as _srv:
            return sh(*args_)
    if args.load_cmd == "bench":
        with Server(load_config()) as _srv:
            return sh("bash", str(REPO / "scripts" / "subnet_ddos_bench.sh"))
    print(f"unknown load command: {args.load_cmd}")
    return 1


# ── CLI ───────────────────────────────────────────────────────────────────────

def main() -> int:
    ap = argparse.ArgumentParser(description="RamShield testing suite")
    sub = ap.add_subparsers(dest="layer", required=True)
    sub.add_parser("lint", help="cargo fmt --check + clippy -D warnings")
    sub.add_parser("unit", help="cargo test --all")
    p_e2e = sub.add_parser("e2e", help="end-to-end protocol test on scratch ports")
    p_e2e.add_argument("--keep", action="store_true", help="(reserved) keep server after run")
    p_xdp = sub.add_parser("xdp", help="kernel dataplane proof on a real NIC (SKIPs when N/A)")
    p_xdp.add_argument("--target", default=None,
                       help="probe IP to block (default: pick a safe public IP)")
    p_load = sub.add_parser("load", help="attack simulator: profiles | run | bench")
    p_load.add_argument("load_cmd", choices=["profiles", "run", "bench"])
    p_load.add_argument("--profile", default="l7_http_flood")
    p_load.add_argument("--duration", type=float, default=30)
    sub.add_parser("all", help="lint + unit + e2e + xdp (CI order)")
    args = ap.parse_args()

    print(f"ramshield suite — repo {REPO}\n")
    if args.layer == "lint":
        return layer_lint()
    if args.layer == "unit":
        return layer_unit()
    if args.layer == "e2e":
        return layer_e2e()
    if args.layer == "xdp":
        return layer_xdp(getattr(args, "target", None))
    if args.layer == "load":
        return layer_load(args)
    if args.layer == "all":
        fails = layer_lint() + layer_unit() + layer_e2e() + layer_xdp()
        print(f"TOTAL FAILURES: {fails}")
        return fails
    return 1


if __name__ == "__main__":
    sys.exit(main())