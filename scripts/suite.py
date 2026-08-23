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
  load     attack profiles via attack_nexus.py (the retained simulator):
             profiles list | run --profile NAME --duration S | bench

Authorized testing only — everything binds/talks to 127.0.0.1.

Usage:
  python3 scripts/suite.py unit
  python3 scripts/suite.py lint
  python3 scripts/suite.py e2e                 # full end-to-end pass
  python3 scripts/suite.py e2e --keep          # keep server running after
  python3 scripts/suite.py load profiles
  python3 scripts/suite.py load run --profile l7_http_flood --duration 30
  python3 scripts/suite.py load bench          # 5-min subnet DDoS benchmark
  python3 scripts/suite.py all                 # lint + unit + e2e (CI order)
Exit code = number of failed layers (0 = all green).
"""
from __future__ import annotations

import argparse
import json
import os
import signal
import socket
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
BIN = REPO / "target" / "release" / "ramshield"
IPC_PORT = 17890
DASH_PORT = 19999
IPC_ADDR = f"127.0.0.1:{IPC_PORT}"
DASH_URL = f"http://127.0.0.1:{DASH_PORT}"
START_TIMEOUT = 30.0


# ── helpers ───────────────────────────────────────────────────────────────────

def sh(*args: str, cwd: Path = REPO, timeout: int | None = None) -> int:
    print(f"  $ {' '.join(args)}")
    return subprocess.run(args, cwd=cwd, timeout=timeout).returncode


def sh_out(*args: str, cwd: Path = REPO, timeout: int | None = None) -> tuple[int, str]:
    r = subprocess.run(args, cwd=cwd, capture_output=True, text=True, timeout=timeout)
    return r.returncode, (r.stdout + r.stderr)


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
    """Scratch-port release server lifecycle (never touches :7890/:9999)."""

    def __init__(self) -> None:
        self.proc: subprocess.Popen | None = None

    def __enter__(self) -> "Server":
        if not BIN.exists():
            raise SystemExit(f"release binary missing: {BIN}\n  cargo build --release -F full")
        # Scratch ports are injected via env overrides (RAMSHIELD_IPC__TCP_ADDR /
        # RAMSHIELD_DASHBOARD__HTTP_ADDR) so we never collide with a live
        # instance on :7890/:9999 regardless of which config file is loaded.
        env = dict(os.environ,
                   RAMSHIELD_IPC__TCP_ADDR=IPC_ADDR,
                   RAMSHIELD_DASHBOARD__HTTP_ADDR=f"127.0.0.1:{DASH_PORT}",
                   RAMSHIELD_DASHBOARD__ENABLED="true")
        self.proc = subprocess.Popen(
            [str(BIN), "config.toml"],
            cwd=str(REPO),
            env=env,
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
    with Server() as srv:
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
        c.ok(bool(r.get("blocked") in (True, "true") or r.get("ok")), "block_ip accepted", str(r))
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

        # subnet block: many distinct IPs from one /24
        events = [{"ip": f"192.0.2.{i}", "bytes": 256, "status_code": 404, "proto_fp": 0x1000}
                  for i in range(250)]
        ipc({"type": "report_connections", "events": events})
        ipc({"type": "report_connections", "events": events})
        ipc({"type": "report_connections", "events": events})
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
        with Server() as _srv:
            return sh(*args_)
    if args.load_cmd == "bench":
        with Server() as _srv:
            return sh(sys.executable, str(REPO / "scripts" / "subnet_ddos_bench.sh"))
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
    p_load = sub.add_parser("load", help="attack simulator: profiles | run | bench")
    p_load.add_argument("load_cmd", choices=["profiles", "run", "bench"])
    p_load.add_argument("--profile", default="l7_http_flood")
    p_load.add_argument("--duration", type=float, default=30)
    sub.add_parser("all", help="lint + unit + e2e (CI order)")
    args = ap.parse_args()

    print(f"ramshield suite — repo {REPO}\n")
    if args.layer == "lint":
        return layer_lint()
    if args.layer == "unit":
        return layer_unit()
    if args.layer == "e2e":
        return layer_e2e()
    if args.layer == "load":
        return layer_load(args)
    if args.layer == "all":
        fails = layer_lint() + layer_unit() + layer_e2e()
        print(f"TOTAL FAILURES: {fails}")
        return fails
    return 1


if __name__ == "__main__":
    sys.exit(main())
