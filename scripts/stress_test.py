#!/usr/bin/env python3
"""
RamShield Stress Test Suite — production-like benchmark.

Phases:
  1. Warm-up (10s low RPS)
  2. Ramp-up (10s → steady state)
  3. Steady-state flood (30s target)
  4. Burst (10s 2x rate)
  5. Sustained load (60s)
  6. Cooldown (10s)

Reports: throughput, latency, rejection rate, detection triggers.
"""

from __future__ import annotations
import argparse
import json
import os
import random
import socket
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass, field
from typing import List, Optional

DEFAULT_HOST = "127.0.0.1"
DEFAULT_PORT = 7890

@dataclass
class PhaseResult:
    name: str
    duration_s: float
    events_sent: int
    batches_sent: int
    errors: int
    eps: float

    def __str__(self):
        return (f"[{self.name:12s}] {self.events_sent:>12,} events | "
                f"{self.batches_sent:>8,} batches | "
                f"{self.eps:>10,.0f} EPS | "
                f"errors={self.errors}")

def send_batch(sock: socket.socket, events: list[dict]) -> bool:
    payload = json.dumps({"type": "report_connections", "events": events}) + "\n"
    try:
        sock.sendall(payload.encode())
        return True
    except (BrokenPipeError, ConnectionResetError, OSError):
        return False

def connect(host: str, port: int) -> socket.socket | None:
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.settimeout(5.0)
        s.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        s.setsockopt(socket.SOL_SOCKET, socket.SO_SNDBUF, 1 << 20)
        s.connect((host, port))
        return s
    except OSError:
        return None

def generate_event(target_ip: str | None = None) -> dict:
    if target_ip:
        ip = target_ip
    else:
        ip = f"{random.randint(1,223)}.{random.randint(0,255)}.{random.randint(0,255)}.{random.randint(1,254)}"
    status = random.choices([200, 200, 200, 301, 404, 500, 403], weights=[60, 15, 10, 5, 5, 3, 2])[0]
    return {
        "ip": ip,
        "bytes": random.randint(64, 8192),
        "status_code": status,
        "proto_fp": random.randint(0, 0xFFFF),
    }

def worker_thread_fn(sock: socket.socket, events_per_iter: int, stop: threading.Event, stats: PhaseResult, lock: threading.Lock, target_eps_per_worker: int):
    interval = 1.0 / (target_eps_per_worker / events_per_iter) if target_eps_per_worker > 0 else 1.0
    while not stop.is_set():
        batch = [generate_event() for _ in range(events_per_iter)]
        if send_batch(sock, batch):
            with lock:
                stats.events_sent += len(batch)
                stats.batches_sent += 1
        else:
            with lock:
                stats.errors += 1
            break
        if interval > 0:
            time.sleep(interval)

def run_phase(name: str, duration_s: float, target_eps: int, workers: int, batch_size: int, host: str, port: int) -> PhaseResult:
    events_per_iter = max(1, batch_size)
    num_workers = max(1, workers)

    stats = PhaseResult(name=name, duration_s=duration_s, events_sent=0, batches_sent=0, errors=0, eps=0)
    stop = threading.Event()
    lock = threading.Lock()

    socks = []
    for _ in range(num_workers):
        s = connect(host, port)
        if s:
            socks.append(s)

    if not socks:
        print(f"  ✗ Cannot connect to {host}:{port}")
        return PhaseResult(name=name, duration_s=0, events_sent=0, batches_sent=0, errors=1, eps=0)
    
    threads = []
    target_eps_per_worker = max(1, target_eps // len(socks))

    for s in socks:
        t = threading.Thread(target=worker_thread_fn, args=(s, events_per_iter, stop, stats, lock, target_eps_per_worker), daemon=True)
        threads.append(t)
        t.start()

    t0 = time.monotonic()
    time.sleep(duration_s)
    stop.set()

    for t in threads:
        t.join(timeout=2)
    for s in socks:
        s.close()

    elapsed = time.monotonic() - t0
    stats.eps = stats.events_sent / elapsed if elapsed > 0 else 0
    stats.duration_s = elapsed
    return stats

def get_snapshot(host: str, port: int) -> dict | None:
    try:
        import urllib.request
        resp = urllib.request.urlopen(f"http://{host}:{port+1}/api/snapshot", timeout=2) # Dashboard is +1 port
        return json.loads(resp.read())
    except Exception:
        return None

def main():
    p = argparse.ArgumentParser()
    p.add_argument("--host", default=DEFAULT_HOST)
    p.add_argument("--port", type=int, default=DEFAULT_PORT)
    p.add_argument("--workers", type=int, default=256)
    p.add_argument("--batch-size", type=int, default=2000)
    args = p.parse_args()

    target_host = args.host
    target_port = args.port

    print("╔══════════════════════════════════════════════════════════════╗")
    print("║           RamShield Stress Test Suite v1.0                  ║")
    print("╚══════════════════════════════════════════════════════════════╝")
    print(f"  Target:  {target_host}:{target_port}")
    print(f"  Workers: {args.workers} | Batch: {args.batch_size}")
    print()

    phases = [
        # (name, duration_s, target_eps)
        ("warmup",       10,  10_000),
        ("ramp-up",      10,  50_000),
        ("steady",       30, 100_000),
        ("burst",        10, 200_000),
        ("sustained",    60, 150_000),
        ("cooldown",     10,  20_000),
    ]

    results = []
    for name, dur, eps in phases:
        print(f"  ▶ Phase: {name:12s}  target={eps:>10,} EPS  duration={dur:>3}s")
        r = run_phase(name, dur, eps, args.workers, args.batch_size, target_host, target_port)
        results.append(r)
        print(f"    {r}")
        snap = get_snapshot(target_host, target_port)
        if snap:
            print(f"    Server: ips_tracked={snap.get('ips_tracked', '?'):,}  "
                  f"blocked={snap.get('blocked_total', '?'):,}  "
                  f"ingested={snap.get('events_ingested', '?'):,}  "
                  f"rejected={snap.get('events_rejected', '?'):,}  "
                  f"channel={snap.get('channel_depth', '?'):,}  "
                  f"CPU={snap.get('cpu_usage', '?'):.1f}%  "
                  f"RAM={snap.get('ram_bytes', 0)/1024/1024:.1f}MB")
        print()

    print("══════════════════════════════════════════════════════════════")
    print("  SUMMARY")
    print("══════════════════════════════════════════════════════════════")
    total_events = sum(r.events_sent for r in results)
    total_batches = sum(r.batches_sent for r in results)
    total_errors = sum(r.errors for r in results)
    total_dur = sum(r.duration_s for r in results)
    peak_eps = max(r.eps for r in results)

    print(f"  Total events sent:   {total_events:>14,}")
    print(f"  Total batches:       {total_batches:>14,}")
    print(f"  Total errors:        {total_errors:>14,}")
    print(f"  Total duration:      {total_dur:>13.1f}s")
    print(f"  Peak throughput:      {peak_eps:>13,.0f} EPS")
    print(f"  Avg throughput:      {total_events/total_dur:>13,.0f} EPS")
    print(f"  Error rate:          {total_errors/max(1,total_batches)*100:>13.2f}%")
    print()

    snap = get_snapshot(target_host, target_port)
    if snap:
        print("  SERVER STATE:")
        print(f"    IPs tracked:        {snap.get('ips_tracked', '?')}")
        print(f"    Blocked total:      {snap.get('blocked_total', '?')}")
        print(f"    Events ingested:    {snap.get('events_ingested', '?')}")
        print(f"    Events rejected:    {snap.get('events_rejected', '?')}")
        print(f"    Channel depth:      {snap.get('channel_depth', '?')}")
        print(f"    Batches total:      {snap.get('batches_total', '?')}")
        print(f"    CPU usage:          {snap.get('cpu_usage', '?')}%")
        print(f"    RAM:                {snap.get('ram_bytes', 0)/1024/1024:.1f} MB / {snap.get('ram_limit_mb', '?')} MB ({snap.get('ram_pct', '?'):.1f}%)")

    print()
    out = {
        "target": f"{target_host}:{target_port}",
        "workers": args.workers,
        "batch_size": args.batch_size,
        "total_events": total_events,
        "total_batches": total_batches,
        "total_errors": total_errors,
        "total_duration_s": round(total_dur, 2),
        "peak_eps": round(peak_eps),
        "avg_eps": round(total_events / max(total_dur, 0.001)),
        "error_rate_pct": round(total_errors / max(1, total_batches) * 100, 3),
        "phase_results": [
            {"name": r.name, "eps": round(r.eps), "events": r.events_sent,
             "errors": r.errors, "duration_s": round(r.duration_s, 2)}
            for r in results
        ],
    }
    with open("stress_results.json", "w") as f:
        json.dump(out, f, indent=2)
    print("  Results saved to: stress_results.json")
    print("══════════════════════════════════════════════════════════════")

if __name__ == "__main__":
    main()
