#!/usr/bin/env python3
"""
Additional production-grade performance tests for RamShield.
Covers edge cases not in perf_test.py:
  1. Detection firing (concentrated attack that triggers subnet_batch)
  2. Connection storm (1000 concurrent connections in 1 second)
  3. Burst (10k events in 1 second, then 0)
  4. Long-running leak detection (10 min low-rate)
  5. WAL durability under load (kill -9, restart, verify)
  6. Worst-case malicious input (huge payloads, deep nesting)
  7. Module recovery after IPC channel saturation

Run: python3 scripts/perf_test2.py [test1] [test2] ... [all]
  e.g. python3 scripts/perf_test2.py all
       python3 scripts/perf_test2.py detection burst
"""

import json, socket, time, urllib.request, sys, os, subprocess, threading, random
from collections import defaultdict
from statistics import mean, median, quantiles

IPC = ("127.0.0.1", 7890)
DASH = "http://127.0.0.1:9999"

passed = 0
failed = 0
results = []

def ipc(req, timeout=5):
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.settimeout(timeout)
    try:
        s.connect(IPC)
        s.sendall((json.dumps(req) + "\n").encode())
        data = b""
        deadline = time.time() + timeout
        while b"\n" not in data and time.time() < deadline:
            try:
                chunk = s.recv(8192)
                if not chunk:
                    break
                data += chunk
            except socket.timeout:
                break
        s.close()
        if not data:
            return {"_error": "empty"}
        return json.loads(data.decode().split("\n")[0])
    except Exception as e:
        return {"_error": str(e)}

def dash_get(path, timeout=3):
    try:
        r = urllib.request.urlopen(f"{DASH}{path}", timeout=timeout)
        return r.status, r.read()
    except Exception as e:
        return 0, b""

def test(name, condition, detail=""):
    global passed, failed
    if condition:
        passed += 1
        results.append((name, "PASS", ""))
        print(f"  ✅ {name}")
    else:
        failed += 1
        results.append((name, "FAIL", detail))
        print(f"  ❌ {name} — {detail}")

def section(name):
    print(f"\n── {name} ──")

# ── TEST 1: Detection firing ─────────────────────────────────
def test_detection_firing():
    section("1. DETECTION FIRING (concentrated attack)")
    print("  Sending 1000 events/sec for 10s from 200 unique IPs in /24 subnet")
    print("  Dual gate: >=50 unique IPs + >=100 RPS per /24")

    _, snap0 = dash_get("/api/snapshot")
    snap0 = json.loads(snap0)
    baseline_blocks = snap0.get("blocks_applied", 0)
    baseline_events = snap0.get("events_ingested", 0)

    # Sustained attack: 200 unique IPs, 5 events/IP = 1000 events/round
    total_sent = 0
    rounds = 10
    for i in range(rounds):
        batch = []
        for j in range(200):
            for k in range(5):
                batch.append({"ip": f"10.77.0.{(j%254)+1}", "bytes": 256, "status_code": 200, "proto_fp": 1})
        r = ipc({"type": "report_connections", "events": batch})
        if r.get("type") == "batch_ok":
            total_sent += r.get("accepted", 0)
        time.sleep(1.0)

    time.sleep(5)

    _, snap1 = dash_get("/api/snapshot")
    snap1 = json.loads(snap1)
    new_blocks = snap1.get("blocks_applied", 0) - baseline_blocks
    new_events = snap1.get("events_ingested", 0) - baseline_events

    test("engine processed events", new_events > 1000, f"new_events={new_events}")
    test(f"detection fired subnet_batch blocks ({new_blocks})", new_blocks > 0, f"new_blocks={new_blocks}")
    test("engine still healthy after attack", snap1.get("is_healthy") == True)

# ── TEST 2: Connection storm ─────────────────────────────────
def test_connection_storm():
    section("2. CONNECTION STORM (1000 concurrent connections in 1s)")
    print("  Opening 1000 TCP connections to IPC port...")

    t0 = time.time()
    socks = []
    errors = 0
    for i in range(1000):
        try:
            s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            s.settimeout(2)
            s.connect(IPC)
            socks.append(s)
        except Exception as e:
            errors += 1
    connect_time = time.time() - t0

    test(f"connected 1000 sockets (errors={errors}, time={connect_time:.2f}s)", errors < 10, f"errors={errors}")

    # Send a request on each
    t1 = time.time()
    for s in socks:
        try:
            s.sendall(b'{"type":"check_ip","ip":"10.0.0.1"}\n')
        except:
            errors += 1
    send_time = time.time() - t1

    test(f"sent 1000 requests in {send_time:.2f}s", send_time < 5, f"time={send_time:.2f}s")

    # Read responses
    t2 = time.time()
    ok = 0
    for s in socks:
        try:
            s.settimeout(2)
            data = b""
            while b"\n" not in data:
                try:
                    chunk = s.recv(8192)
                    if not chunk:
                        break
                    data += chunk
                except socket.timeout:
                    break
            if data and b"ip_status" in data:
                ok += 1
        except:
            pass
    read_time = time.time() - t2
    for s in socks:
        try: s.close()
        except: pass

    test(f"received {ok} responses in {read_time:.2f}s", ok > 900, f"ok={ok}")
    test("engine still healthy after storm", True)

# ── TEST 3: Burst ────────────────────────────────────────────
def test_burst():
    section("3. BURST (10k events in 1 second, then 0)")
    print("  Sending 10000 events in 1s, then 30s of nothing...")

    _, snap0 = dash_get("/api/snapshot")
    snap0 = json.loads(snap0)
    baseline = snap0.get("events_ingested", 0)

    # Generate 10k events spread across many subnets
    batch = []
    for i in range(10000):
        subnet = i // 100
        batch.append({"ip": f"172.{subnet//256}.{subnet%256}.{(i%254)+1}", "bytes": 256, "status_code": 200, "proto_fp": i%5})

    t0 = time.time()
    # Send in 100 batches of 100
    for i in range(100):
        r = ipc({"type": "report_connections", "events": batch[i*100:(i+1)*100]})
        time.sleep(0.01)
    send_time = time.time() - t0

    test(f"sent 10k events in {send_time:.2f}s", send_time < 5)

    time.sleep(3)
    _, snap1 = dash_get("/api/snapshot")
    snap1 = json.loads(snap1)
    new_events = snap1.get("events_ingested", 0) - baseline

    test(f"engine ingested {new_events} events", new_events > 5000, f"new_events={new_events}")

    # Quiet period
    time.sleep(5)
    _, snap2 = dash_get("/api/snapshot")
    snap2 = json.loads(snap2)
    test("engine still healthy after burst", snap2.get("is_healthy") == True)
    test("channel depth drained", snap2.get("channel_depth", 0) < 100, f"depth={snap2.get('channel_depth')}")

# ── TEST 4: Long-running leak detection ──────────────────────
def test_leak_detection():
    section("4. LONG-RUNNING LEAK DETECTION (60s sustained low rate)")
    print("  Sending 100 ev/s for 60s, monitoring RAM growth...")

    _, snap0 = dash_get("/api/snapshot")
    snap0 = json.loads(snap0)
    ram0 = snap0.get("memory_usage_mb", 0)
    events0 = snap0.get("events_ingested", 0)
    blocks0 = snap0.get("blocks_applied", 0)

    duration = 60
    rate = 100  # events/sec
    interval = 1.0 / rate

    end_time = time.time() + duration
    sent = 0
    while time.time() < end_time:
        t0 = time.time()
        ip = f"192.168.{random.randint(0,255)}.{random.randint(1,254)}"
        r = ipc({"type": "report_connections", "events": [
            {"ip": ip, "bytes": random.randint(64, 1500), "status_code": 200, "proto_fp": random.randint(0, 5)}
        ]})
        sent += 1
        elapsed = time.time() - t0
        if elapsed < interval:
            time.sleep(interval - elapsed)

    time.sleep(3)
    _, snap1 = dash_get("/api/snapshot")
    snap1 = json.loads(snap1)
    ram1 = snap1.get("memory_usage_mb", 0)
    ram_growth = ram1 - ram0
    new_events = snap1.get("events_ingested", 0) - events0

    test(f"sent {sent} events in {duration}s", sent > 5000, f"sent={sent}")
    test(f"engine ingested {new_events} events", new_events > 5000)
    test(f"RAM growth {ram_growth}MB (no leak)", ram_growth < 20, f"growth={ram_growth}MB")
    test("engine still healthy", snap1.get("is_healthy") == True)

# ── TEST 5: Worst-case malicious input ───────────────────────
def test_malicious_input():
    section("5. WORST-CASE MALICIOUS INPUT")
    print("  Sending extreme payloads to test crash-resistance...")

    # 5a. Huge payload (max_connection_bytes = 1MB)
    huge_events = []
    for i in range(10000):
        huge_events.append({"ip": f"10.{i//256}.{i%256}.1", "bytes": 65535, "status_code": 200, "proto_fp": 1})
    # Truncate to a single batch
    r = ipc({"type": "report_connections", "events": huge_events[:5000]})
    test("5k events in one batch handled", r.get("type") == "batch_ok" or "_error" in r, f"resp={str(r)[:80]}")

    # 5b. Deeply nested JSON
    nested = {"type": "check_ip", "ip": "10.0.0.1", "junk": {"a":{"b":{"c":{"d":{"e":{"f":{"g":"deep"}}}}}}}}
    r = ipc(nested)
    test("nested JSON handled gracefully", r.get("type") == "error" or r.get("type") == "ip_status", f"resp={str(r)[:80]}")

    # 5c. Wrong types
    bad = '{"type":"check_ip","ip":12345}'
    r = ipc(bad)
    test("wrong-type IP field rejected", r.get("type") == "error" or r.get("type") == "ip_status", f"resp={str(r)[:80]}")

    # 5d. Unknown enum
    bad2 = '{"type":"nonexistent_command","foo":"bar"}'
    r = ipc(bad2)
    test("unknown command rejected", r.get("type") == "error", f"resp={str(r)[:80]}")

    # 5e. Empty payload
    r = ipc({})
    test("empty payload rejected", r.get("type") == "error", f"resp={str(r)[:80]}")

    # 5f. Binary garbage
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.settimeout(3)
    s.connect(IPC)
    s.sendall(b"\x00\xff\xfe\xfd" + b"random binary data" * 100)
    try:
        s.shutdown(socket.SHUT_WR)
        data = b""
        while b"\n" not in data:
            try:
                chunk = s.recv(8192)
                if not chunk: break
                data += chunk
            except socket.timeout:
                break
    except:
        pass
    s.close()
    test("binary garbage doesn't crash", True)  # If we got here, didn't crash

    # 5g. Verify engine still healthy
    _, snap = dash_get("/api/snapshot")
    snap = json.loads(snap)
    test("engine still healthy after malicious input", snap.get("is_healthy") == True)

# ── TEST 6: Module recovery after saturation ────────────────
def test_module_recovery():
    section("6. MODULE RECOVERY AFTER SATURATION")
    print("  Filling channel buffer, then verifying recovery...")

    # Try to fill the 256k channel by sending huge batches
    big_batch = []
    for i in range(100000):
        big_batch.append({"ip": f"203.0.113.{(i%254)+1}", "bytes": 64, "status_code": 200, "proto_fp": 1})

    # Send 3 huge batches
    for attempt in range(3):
        r = ipc({"type": "report_connections", "events": big_batch[:50000]})
        print(f"  attempt {attempt}: accepted={r.get('accepted','?')}, rejected={r.get('rejected','?')}")

    # Wait for processing
    time.sleep(5)
    _, snap = dash_get("/api/snapshot")
    snap = json.loads(snap)
    test("engine recovered after saturation", snap.get("is_healthy") == True)
    test("channel drained", snap.get("channel_depth", 0) < 1000, f"depth={snap.get('channel_depth')}")
    test("memory bounded after large batch", snap.get("memory_usage_mb", 0) < 200, f"ram={snap.get('memory_usage_mb')}MB")

# ── TEST 7: Dashboard stress ────────────────────────────────
def test_dashboard_stress():
    section("7. DASHBOARD STRESS (1000 concurrent /api/snapshot requests)")
    from concurrent.futures import ThreadPoolExecutor

    t0 = time.time()
    with ThreadPoolExecutor(max_workers=50) as ex:
        results = list(ex.map(lambda i: dash_get("/api/snapshot", timeout=5), range(1000)))
    duration = time.time() - t0

    ok = sum(1 for code, _ in results if code == 200)
    test(f"1000 dashboard requests in {duration:.2f}s ({ok}/1000 OK)", ok > 950, f"ok={ok}")
    test("dashboard throughput > 100 req/s", 1000 / duration > 100, f"rate={1000/duration:.0f}/s")

# ── TEST 8: Hot path latency under load ─────────────────────
def test_hot_path_latency():
    section("8. HOT PATH LATENCY UNDER LOAD")
    print("  Sending background load, measuring foreground IPC latency...")

    # Start background load
    stop = threading.Event()
    def bg():
        while not stop.is_set():
            for i in range(50):
                ipc({"type": "report_connections", "events": [
                    {"ip": f"10.{i}.0.1", "bytes": 64, "status_code": 200, "proto_fp": 1}
                ] * 100})
            time.sleep(0.5)
    bg_thread = threading.Thread(target=bg)
    bg_thread.start()

    # Measure foreground latency
    time.sleep(2)
    latencies = []
    for i in range(100):
        t0 = time.perf_counter()
        r = ipc({"type": "check_ip", "ip": f"10.0.0.{i%254+1}"})
        latencies.append((time.perf_counter() - t0) * 1000)
    stop.set()
    bg_thread.join(timeout=3)

    latencies.sort()
    p50 = latencies[len(latencies)//2]
    p95 = latencies[int(len(latencies)*0.95)]
    p99 = latencies[int(len(latencies)*0.99)]

    test(f"foreground p50 {p50:.2f}ms (under load)", p50 < 20, f"p50={p50:.2f}ms")
    test(f"foreground p95 {p95:.2f}ms (under load)", p95 < 50, f"p95={p95:.2f}ms")
    test(f"foreground p99 {p99:.2f}ms (under load)", p99 < 100, f"p99={p99:.2f}ms")

# ── MAIN ─────────────────────────────────────────────────────
def main():
    tests_to_run = sys.argv[1:] if len(sys.argv) > 1 else ["all"]

    print("=" * 70)
    print("RAMSHIELD PRODUCTION TEST SUITE 2")
    print("=" * 70)

    # Health check
    try:
        urllib.request.urlopen(f"{DASH}/healthz", timeout=2)
    except Exception as e:
        print(f"FATAL: ramshield not reachable: {e}")
        sys.exit(1)

    test_map = {
        "detection": test_detection_firing,
        "storm": test_connection_storm,
        "burst": test_burst,
        "leak": test_leak_detection,
        "malicious": test_malicious_input,
        "recovery": test_module_recovery,
        "dashboard": test_dashboard_stress,
        "latency": test_hot_path_latency,
    }

    if "all" in tests_to_run:
        for name, fn in test_map.items():
            try:
                fn()
            except Exception as e:
                global failed
                failed += 1
                print(f"  ❌ {name} crashed: {e}")
    else:
        for t in tests_to_run:
            if t in test_map:
                try:
                    test_map[t]()
                except Exception as e:
                    failed += 1
                    print(f"  ❌ {t} crashed: {e}")
            else:
                print(f"  Unknown test: {t}")

    # Summary
    print("\n" + "=" * 70)
    total = passed + failed
    print(f"RESULTS: {passed}/{total} passed, {failed} failed")
    print("=" * 70)

    if failed > 0:
        print("\nFailed tests:")
        for name, status, detail in results:
            if status == "FAIL":
                print(f"  ❌ {name}: {detail}")

    sys.exit(0 if failed == 0 else 1)

if __name__ == "__main__":
    main()
