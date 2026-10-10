#!/usr/bin/env python3
"""
Subnet diversity test - generate many unique /24 subnets.
Goal: see hot_subnets_count and subnet_bitmap_ones grow in dashboard.
"""
import socket
import json
import time
import threading
import urllib.request
import random

IPC_HOST = "127.0.0.1"
IPC_PORT = 7890
BATCH_SIZE = 25       # smaller batches
FRAME_DELAY = 0.05    # 20 frames/sec → 500 events/s per thread
DURATION = 120        # 2 minutes
NUM_THREADS = 2       # fewer threads to reduce connection churn


def build_frame(batch_start):
    """Create one IPC frame with unique IPs from sequential subnets."""
    events = []
    for i in range(BATCH_SIZE):
        n = batch_start + i
        # Use high bits for subnet diversity, low bits for host
        # This creates IPs like: 10.0.0.1, 10.0.1.1, 10.0.2.1, ... then 10.1.0.1, etc.
        subnet = n
        host = (n >> 16) & 0xFF + 1  # ensure host 1-254
        a = (subnet >> 8) & 0xFF
        b = subnet & 0xFF
        ip = f"10.{a}.{b}.{host}"
        events.append({
            "ip": ip,
            "bytes": random.randint(256, 4096),
            "status_code": random.choice([200, 200, 200, 404]),
            "proto_fp": random.choice([6, 17])  # TCP/UDP mostly
        })
    return json.dumps({"type": "report_connections", "events": events}) + "\n"


def sender(thread_id, start_n, total_frames, stats):
    """Send frames continuously."""
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)

    frames_sent = 0
    reconnects = 0
    n = start_n
    start = time.monotonic()

    while frames_sent < total_frames:
        try:
            s.connect((IPC_HOST, IPC_PORT))
            break
        except Exception:
            reconnects += 1
            if reconnects > 5:
                print(f"  Thread {thread_id}: giving up after {reconnects} reconnects")
                return
            time.sleep(0.5 * reconnects)

    while frames_sent < total_frames:
        try:
            frame = build_frame(n)
            s.sendall(frame.encode())
            frames_sent += 1
            n += BATCH_SIZE
            stats['frames'] += 1
            time.sleep(FRAME_DELAY)
        except (BrokenPipeError, ConnectionResetError):
            # Reconnect on network error
            s.close()
            time.sleep(0.1)
            s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            s.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
            continue
        except Exception as e:
            print(f"  Thread {thread_id}: error: {e}")
            break

    s.close()
    stats['elapsed'] = time.monotonic() - start
    stats['reconnects'] = reconnects


def monitor():
    """Print dashboard stats every 10 seconds."""
    print(f"\n{'time':>6} {'ips':>10} {'ingested':>12} {'subnets':>10} {'hot':>8} {'cpu':>6} {'health':>8}")
    print(f"{'-'*70}")

    for i in range(0, DURATION, 10):
        time.sleep(10)
        try:
            data = json.loads(urllib.request.urlopen(
                f"http://{IPC_HOST}:9999/api/snapshot", timeout=3
            ).read())
            print(f"  t+{i+10:3d}s "
                  f"{data.get('ips_tracked', 0):>10,} "
                  f"{data.get('events_ingested', 0):>12,} "
                  f"{data.get('subnet_bitmap_ones', 0):>10,} "
                  f"{data.get('hot_subnets_count', 0):>8,} "
                  f"{data.get('cpu_usage_pct', 0):>5.1f}% "
                  f"{data.get('is_healthy', False)!s:>8}")
        except Exception as e:
            print(f"  t+{i+10:3d}s error: {e}")


print("Subnet Diversity Test")
print("=" * 70)
print(f"Each thread sends {BATCH_SIZE} events/frame every {FRAME_DELAY*1000:.0f}ms")
print(f"Duration: {DURATION}s across {NUM_THREADS} threads")
print(f"Target subnets: ~{BATCH_SIZE * (DURATION / FRAME_DELAY) * NUM_THREADS:,.0f}")
print(f"=" * 70)

stats = {'frames': 0, 'elapsed': 0, 'reconnects': 0}
threads = []
frames_per_thread = int(DURATION / FRAME_DELAY) // NUM_THREADS

for i in range(NUM_THREADS):
    start_n = i * frames_per_thread * BATCH_SIZE * 1000  # large offset to avoid overlap
    t = threading.Thread(target=sender, args=(i, start_n, frames_per_thread, stats), daemon=True)
    threads.append(t)
    t.start()
    print(f"  Thread {i} started (starting at subnet ~{start_n:,})")

# Start monitor
monitor_thread = threading.Thread(target=monitor, daemon=True)
monitor_thread.start()

# Wait for completion
for t in threads:
    t.join()

print(f"\n{'='*70}")
print(f"Test Complete")
print(f"{'='*70}")
print(f"Frames sent: {stats['frames']:,}")
print(f"Avg reconnects per thread: {stats['reconnects'] / NUM_THREADS if NUM_THREADS > 0 else 0:.1f}")
print(f"Total time: {stats['elapsed']:.1f}s")

# Final snapshot
try:
    data = json.loads(urllib.request.urlopen(
        f"http://{IPC_HOST}:9999/api/snapshot", timeout=3
    ).read())
    print(f"\nFinal State:")
    print(f"  IPs tracked: {data.get('ips_tracked', 0):,}")
    print(f"  Events ingested: {data.get('events_ingested', 0):,}")
    print(f"  Subnet bitmap ones: {data.get('subnet_bitmap_ones', 0):,} (unique /24 subnets seen)")
    print(f"  Hot subnets: {data.get('hot_subnets_count', 0):,} (active subnets)")
    print(f"  Health: {data.get('is_healthy', False)}")
except Exception as e:
    print(f"  Final error: {e}")

print("=" * 70)
