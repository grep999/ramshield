#!/usr/bin/env python3
"""
RamShield load generator — 100k-event attack simulation.

Floods the IPC TCP interface with connection reports to stress the daemon,
network stack, and batch detection pipeline. Uses report_connections batches
(high-throughput path) with optional multi-socket concurrency.

Usage:
  # Start RamShield first:
  #   ./target/release/ramshield config.toml

  python3 scripts/attack_sim_100k.py
  python3 scripts/attack_sim_100k.py --events 1000000000000 --workers 32 --batch-size 111500
  python3 scripts/attack_sim_100k.py --mode volumetric --target-ip 10.0.0.99
"""

from __future__ import annotations

import argparse
import json
import random
import socket
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass, field
from typing import Iterable, List, Optional

DEFAULT_HOST = "127.0.0.1"
DEFAULT_PORT = 7890
DEFAULT_EVENTS = 100_000
DEFAULT_WORKERS = 128 
DEFAULT_BATCH = 1500


@dataclass
class Stats:
    sent_events: int = 0
    sent_batches: int = 0
    errors: int = 0
    lock: threading.Lock = field(default_factory=threading.Lock)

    def add(self, events: int, batches: int = 1, errors: int = 0) -> None:
        with self.lock:
            self.sent_events += events
            self.sent_batches += batches
            self.errors += errors


def make_ip(mode: str, rng: random.Random, target_ip: Optional[str], worker_id: int) -> str:
    if mode == "volumetric" and target_ip:
        return target_ip
    if mode == "subnet":
        # Concentrate on one /24 to trigger subnet-scale detection
        return f"203.0.113.{rng.randint(1, 254)}"
    if mode == "botnet":
        return f"{rng.randint(1, 223)}.{rng.randint(0, 255)}.{rng.randint(0, 255)}.{rng.randint(1, 254)}"
    # mixed: blend hot IP, hot subnet, and random sources
    roll = rng.random()
    if roll < 0.15 and target_ip:
        return target_ip
    if roll < 0.35:
        return f"198.51.100.{rng.randint(1, 254)}"
    return f"{rng.randint(1, 223)}.{rng.randint(0, 255)}.{rng.randint(0, 255)}.{rng.randint(1, 254)}"


def make_event(mode: str, rng: random.Random, target_ip: Optional[str], worker_id: int) -> dict:
    if mode == "errors":
        status = rng.choice([404, 403, 500, 502])
    else:
        status = 200 if rng.random() > 0.05 else rng.choice([404, 500])

    return {
        "ip": make_ip(mode, rng, target_ip, worker_id),
        "bytes": rng.randint(64, 4096),
        "status_code": status,
        "proto_fp": rng.randint(0, 0xFFFF),
    }


def chunk_events(events: List[dict], size: int) -> Iterable[List[dict]]:
    for i in range(0, len(events), size):
        yield events[i : i + size]


def send_batch(sock: socket.socket, events: List[dict], read_response: bool) -> Optional[str]:
    payload = json.dumps({"type": "report_connections", "events": events}, separators=(",", ":"))
    sock.sendall((payload + "\n").encode())
    if not read_response:
        return None
    sock.settimeout(2.0)
    chunks: List[bytes] = []
    while True:
        part = sock.recv(4096)
        if not part:
            break
        chunks.append(part)
        if b"\n" in part:
            break
    return b"".join(chunks).decode(errors="replace").strip()


def worker(
    worker_id: int,
    host: str,
    port: int,
    event_count: int,
    batch_size: int,
    mode: str,
    target_ip: Optional[str],
    read_response: bool,
    stats: Stats,
) -> None:
    rng = random.Random(worker_id ^ int(time.time() * 1e6))
    events = [make_event(mode, rng, target_ip, worker_id) for _ in range(event_count)]

    try:
        sock = socket.create_connection((host, port), timeout=5.0)
        sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    except OSError as e:
        stats.add(0, 0, errors=1)
        print(f"[worker {worker_id}] connect failed: {e}", file=sys.stderr)
        return

    try:
        for batch in chunk_events(events, batch_size):
            try:
                send_batch(sock, batch, read_response)
                stats.add(len(batch), batches=1)
            except OSError as e:
                stats.add(0, 0, errors=1)
                print(f"[worker {worker_id}] send failed: {e}", file=sys.stderr)
                break
    finally:
        sock.close()


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description="Simulate 100k-event attack against RamShield IPC")
    p.add_argument("--host", default=DEFAULT_HOST)
    p.add_argument("--port", type=int, default=DEFAULT_PORT)
    p.add_argument("--events", type=int, default=DEFAULT_EVENTS, help="Total events to send")
    p.add_argument("--workers", type=int, default=DEFAULT_WORKERS, help="Parallel TCP connections")
    p.add_argument("--batch-size", type=int, default=DEFAULT_BATCH, help="Events per report_connections message")
    p.add_argument(
        "--mode",
        choices=["mixed", "volumetric", "botnet", "subnet", "errors"],
        default="mixed",
        help="Traffic pattern",
    )
    p.add_argument("--target-ip", default="10.255.0.1", help="Hot source IP for volumetric/mixed modes")
    p.add_argument("--read-response", action="store_true", help="Wait for IPC response (slower)")
    p.add_argument("--dry-run", action="store_true", help="Print plan only, do not connect")
    return p.parse_args()


def main() -> int:
    args = parse_args()

    if args.events < 1:
        print("events must be >= 1", file=sys.stderr)
        return 1
    if args.workers < 1:
        print("workers must be >= 1", file=sys.stderr)
        return 1
    if args.batch_size < 1:
        print("batch-size must be >= 1", file=sys.stderr)
        return 1

    per_worker = args.events // args.workers
    remainder = args.events % args.workers
    counts = [per_worker + (1 if i < remainder else 0) for i in range(args.workers)]
    batches_total = sum((c + args.batch_size - 1) // args.batch_size for c in counts)

    print("=== RamShield attack simulation ===")
    print(f"Target:        {args.host}:{args.port}")
    print(f"Total events:  {args.events:,}")
    print(f"Workers:       {args.workers}")
    print(f"Batch size:    {args.batch_size}")
    print(f"Batches (~):   {batches_total:,}")
    print(f"Mode:          {args.mode}")
    print(f"Target IP:     {args.target_ip}")
    print(f"Read response: {args.read_response}")
    print()

    if args.dry_run:
        return 0

    stats = Stats()
    t0 = time.perf_counter()

    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures = [
            pool.submit(
                worker,
                i,
                args.host,
                args.port,
                counts[i],
                args.batch_size,
                args.mode,
                args.target_ip,
                args.read_response,
                stats,
            )
            for i in range(args.workers)
        ]
        for fut in as_completed(futures):
            fut.result()

    elapsed = time.perf_counter() - t0
    eps = stats.sent_events / elapsed if elapsed > 0 else 0.0

    print("--- results ---")
    print(f"Events sent:   {stats.sent_events:,} / {args.events:,}")
    print(f"Batches sent:  {stats.sent_batches:,}")
    print(f"Errors:        {stats.errors}")
    print(f"Elapsed:       {elapsed:.2f}s")
    print(f"Throughput:    {eps:,.0f} events/s")
    print()
    print("Check blocks/stats:")
    print(f"  ./target/release/ramshield-cli stats")
    print(f"  ./target/release/ramshield-cli check {args.target_ip}")

    return 0 if stats.sent_events > 0 and stats.errors < args.workers else 1


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except KeyboardInterrupt:
        print("\n[!] Aborted.", file=sys.stderr)
        raise SystemExit(130)
