#!/usr/bin/env python3
"""RamShield sustained attack driver.

Attack phases, each 30 min (total 2h):
  1. SYN-flood style: high-rate unique IPs (bloom + store growth pressure)
  2. Subnet flood: concentrated /24 clusters (subnet aggregation path)
  3. Slow-connector mix: low-rate many IPs (TTL/expiry churn)
  4. Block churn: repeated offenders crossing rps_threshold (WAL + enforcement)

Monitors: RSS every 10s -> /tmp/rs_attack_metrics.csv
"""
import json
import os
import socket
import subprocess
import sys
import time

IPC = ("127.0.0.1", 19847)
DASH = "http://127.0.0.1:19848"
BIN = "/home/m/vehicle_of_rationalism/ramshield/beta/rs/target/release/ramshield"
CFG = "/tmp/rs_attack.toml"
LOG = "/tmp/rs_attack_server.log"
METRICS = "/tmp/rs_attack_metrics.csv"

PHASE_SECS = int(os.environ.get("RS_PHASE_SECS", "1800"))  # 30 min each; 4 phases = 2h


def write_cfg():
    with open(CFG, "w") as f:
        f.write("""[engine]
ram_limit_mb = 256
worker_threads = 0
shard_count = 64
[detection]
rps_threshold = 500
rate_window_secs = 1
subnet_batch_threshold = 200
batch_block_enabled = true
block_ttl_secs = 60
bloom_bits = 1048576
batch_max_events = 50000
batch_window_ms = 100
pre_aggs_flush_interval_ms = 250
promote_min_events = 5
subnet_window_threshold = 50
pre_aggs_max_size = 262144
[xdp]
enabled = false
interface = "eth0"
mode = "skb"
[ipc]
tcp_addr = "127.0.0.1:19847"
max_connections = 128
max_connection_bytes = 16777216
[forecasting]
enabled = true
ewma_alpha = 0.3
hw_beta = 0.1
hw_gamma = 0.1
seasonality_period = 60
anomaly_zscore = 3.0
min_entropy = 1.5
[dashboard]
enabled = true
http_addr = "127.0.0.1:19848"
""")


def rss_kb(pid):
    try:
        with open(f"/proc/{pid}/status") as f:
            for line in f:
                if line.startswith("VmRSS:"):
                    return int(line.split()[1])
    except OSError:
        return -1
    return -1


def send_events(events):
    """One IPC connection per batch; returns (accepted, rejected)."""
    msg = json.dumps({"type": "report_connections", "events": events})
    try:
        s = socket.create_connection(IPC, timeout=5)
        s.sendall((msg + "\n").encode())
        buf = b""
        while b"\n" not in buf:
            chunk = s.recv(65536)
            if not chunk:
                break
            buf += chunk
        s.close()
        resp = json.loads(buf.decode().strip())
        return resp.get("accepted", 0), resp.get("rejected", 0)
    except (OSError, json.JSONDecodeError) as e:
        print(f"[!] ipc error: {e}", flush=True)
        return 0, 0


def phase_syn_flood(t_end):
    """Unique IPs at high rate — bloom filter + store insertion pressure."""
    sent = 0
    i = 0
    while time.time() < t_end:
        events = [
            {"ip": f"45.{(i >> 8) & 255}.{i & 255}.{(i * 7) & 255}",
             "bytes": 40, "status_code": 404, "proto_fp": 1}
            for i in range(i, i + 4000)
        ]
        i += 4000
        a, r = send_events(events)
        sent += a
        time.sleep(0.05)  # ~80k events/s ceiling
    return sent


def phase_subnet_flood(t_end):
    """Concentrated /24s — subnet aggregation + batch block path."""
    sent = 0
    round_ = 0
    while time.time() < t_end:
        events = []
        for sub in range(12):
            net = f"91.{round_ % 250}.{sub}."
            for h in range(250):
                events.append({"ip": net + str(h), "bytes": 120,
                               "status_code": 200, "proto_fp": 3})
        round_ += 1
        a, r = send_events(events)
        sent += a
        time.sleep(0.05)
    return sent


def phase_slow_connectors(t_end):
    """Many IPs below promote threshold — cold-skip + TTL churn."""
    sent = 0
    base = 0
    while time.time() < t_end:
        events = [
            {"ip": f"77.{(base >> 16) & 255}.{(base >> 8) & 255}.{base & 255}",
             "bytes": 64, "status_code": 200, "proto_fp": 9}
            for base in range(base, base + 3000)
        ]
        base += 3000
        a, r = send_events(events)
        sent += a
        time.sleep(0.25)
    return sent


def phase_block_churn(t_end):
    """Few IPs hammering over threshold — repeated blocks, WAL writes."""
    sent = 0
    while time.time() < t_end:
        events = []
        for ip_i in range(20):
            ip = f"203.0.{ip_i}.66"
            for _ in range(600):  # 600 ev/IP/batch → way over threshold
                events.append({"ip": ip, "bytes": 512,
                               "status_code": 200, "proto_fp": 2})
        a, r = send_events(events)
        sent += a
        time.sleep(0.5)
    return sent


PHASES = [
    ("syn_flood", phase_syn_flood),
    ("subnet_flood", phase_subnet_flood),
    ("slow_connectors", phase_slow_connectors),
    ("block_churn", phase_block_churn),
]


def main():
    only = sys.argv[1] if len(sys.argv) > 1 else None
    write_cfg()
    proc = subprocess.Popen(
        [BIN, "--config", CFG],
        stdout=open(LOG, "w"), stderr=subprocess.STDOUT,
    )
    time.sleep(2)
    if proc.poll() is not None:
        print("server failed to start", open(LOG).read()[-500:])
        sys.exit(1)

    with open(METRICS, "w") as mf:
        mf.write("ts_phase,elapsed_s,rss_kb\n")
        start = time.time()
        try:
            for name, fn in PHASES:
                if only and name != only:
                    continue
                t_end = time.time() + PHASE_SECS
                print(f"[phase {name}] until +{PHASE_SECS}s", flush=True)
                # monitor thread inline via loop in sender? simpler: sample here
                last_sample = 0
                # run phase with sampling interleaved
                import threading
                stop = threading.Event()

                def sampler():
                    while not stop.is_set():
                        mf.write(f"{name},{time.time()-start:.0f},{rss_kb(proc.pid)}\n")
                        mf.flush()
                        time.sleep(10)

                th = threading.Thread(target=sampler, daemon=True)
                th.start()
                total = fn(t_end)
                stop.set()
                th.join(timeout=15)
                print(f"[phase {name}] sent={total} rss={rss_kb(proc.pid)}kB", flush=True)
        finally:
            time.sleep(1)
            final_rss = rss_kb(proc.pid)
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
            print(f"final rss: {final_rss} kB", flush=True)


if __name__ == "__main__":
    main()
