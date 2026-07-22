#!/usr/bin/env python3
"""
Cruel RamShield DDoS Simulation — 10,000,000 events.

Combines volumetric, subnet, and mixed attack patterns with
high concurrency via the report_connections IPC batch protocol.
"""
import argparse
import json
import random
import socket
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from typing import List, Optional


@dataclass
class Stats:
    sent_events: int = 0
    errors: int = 0
    lock: threading.Lock = field(default_factory=threading.Lock)

    def add(self, events: int, errors: int = 0) -> None:
        with self.lock:
            self.sent_events += events
            self.errors += errors


def make_ip(mode: str, rng: random.Random, target_ip: str, worker_id: int) -> str:
    if mode == "volumetric":
        return target_ip
    if mode == "subnet":
        return f"10.255.{rng.randint(0, 255)}.{rng.randint(1, 254)}"
    # mixed: blend volumetric, subnet, and random
    roll = rng.random()
    if roll < 0.2:
        return target_ip
    if roll < 0.5:
        return f"198.51.100.{rng.randint(1, 254)}"
    return f"{rng.randint(1, 223)}.{rng.randint(0, 255)}.{rng.randint(0, 255)}.{rng.randint(1, 254)}"


def make_event(mode: str, rng: random.Random, target_ip: str, worker_id: int) -> dict:
    return {
        "ip": make_ip(mode, rng, target_ip, worker_id),
        "bytes": rng.randint(64, 4096),
        "status_code": 200 if rng.random() > 0.05 else rng.choice([404, 500]),
        "proto_fp": rng.randint(0, 0xFFFF),
    }


def worker(
    worker_id: int,
    host: str,
    port: int,
    events_per_worker: int,
    batch_size: int,
    stats: Stats,
    target_ip: str,
):
    rng = random.Random(worker_id)
    modes = ["volumetric", "subnet", "mixed"]
    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    try:
        sock.connect((host, port))
        sent = 0
        while sent < events_per_worker:
            n = min(batch_size, events_per_worker - sent)
            batch = [make_event(rng.choice(modes), rng, target_ip, worker_id) for _ in range(n)]
            payload = json.dumps(
                {"type": "report_connections", "events": batch},
                separators=(",", ":"),
            )
            sock.sendall((payload + "\n").encode())
            sent += n
            # drain response
            try:
                sock.settimeout(2.0)
                data = sock.recv(4096)
            except socket.timeout:
                pass
        stats.add(sent)
    except Exception as e:
        stats.add(0, errors=1)
        print(f"Worker {worker_id} error: {e}")
    finally:
        sock.close()


def main():
    parser = argparse.ArgumentParser(description="Cruel RamShield DDoS Simulation")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=7890)
    parser.add_argument("--total-events", type=int, default=10_000_000)
    parser.add_argument("--workers", type=int, default=64)
    parser.add_argument("--batch-size", type=int, default=4096)
    parser.add_argument("--target-ip", default="10.255.0.1")
    args = parser.parse_args()

    print(f"=== Cruel RamShield DDoS Simulation ===")
    print(f"Target:        {args.host}:{args.port}")
    print(f"Total events:  {args.total_events:,}")
    print(f"Workers:       {args.workers}")
    print(f"Batch size:    {args.batch_size}")

    events_per_worker = args.total_events // args.workers
    stats = Stats()

    start = time.time()
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures = [
            pool.submit(worker, i, args.host, args.port, events_per_worker, args.batch_size, stats, args.target_ip)
            for i in range(args.workers)
        ]
        for f in futures:
            f.result()

    elapsed = time.time() - start
    print(f"\n--- results ---")
    print(f"Events sent:   {stats.sent_events:,} / {args.total_events:,}")
    print(f"Errors:        {stats.errors}")
    print(f"Elapsed:       {elapsed:.2f}s")
    if elapsed > 0:
        print(f"Throughput:    {stats.sent_events / elapsed:,.0f} events/s")
    else:
        print(f"Throughput:    N/A")


if __name__ == "__main__":
    main()
