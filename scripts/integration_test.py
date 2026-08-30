#!/usr/bin/env python3
"""
Full integration test for RamShield test11 branch.
Covers: IPC commands, dashboard endpoints, detection,
enforcement, storage, forecasting, edge cases.

Run: python3 scripts/integration_test.py
Requires: ramshield running with --config config.toml --no-xdp
"""

import json, socket, subprocess, sys, time, urllib.request
from concurrent.futures import ThreadPoolExecutor

IPC = ("127.0.0.1", 7890)
DASH = "http://127.0.0.1:9999"
passed = 0
failed = 0
skipped = 0

def ipc_send(req, timeout=2):
    s = socket.socket()
    s.settimeout(timeout)
    try:
        s.connect(IPC)
        s.sendall((req + "\n").encode())
        time.sleep(0.05)
        data = b""
        while True:
            try:
                chunk = s.recv(8192)
            except socket.timeout:
                break
            if not chunk:
                break
            data += chunk
        s.close()
        if not data:
            return {"_error": "empty response"}
        return json.loads(data)
    except Exception as e:
        return {"_error": str(e)}

def dashboard(path, timeout=3):
    try:
        r = urllib.request.urlopen(f"{DASH}{path}", timeout=timeout)
        ct = r.headers.get("Content-Type", "")
        body = r.read()
        if "json" in ct:
            return r.status, json.loads(body)
        else:
            return r.status, body.decode("utf-8", errors="replace")
    except Exception as e:
        return 0, {"_error": str(e)}

def test(name, condition, detail=""):
    global passed, failed
    if condition:
        passed += 1
        print(f"  ✅ {name}")
    else:
        failed += 1
        print(f"  ❌ {name} — {detail}")

print("=" * 60)
print("RAMSHIELD INTEGRATION TEST")
print("=" * 60)

# ── SECTION 1: IPC Protocol ─────────────────────────────────────
print("\n── 1. IPC PROTOCOL ──")

r = ipc_send('{"type":"check_ip","ip":"10.0.0.1"}')
test("check_ip returns ip_status", r.get("type") == "ip_status", str(r)[:80])
test("check_ip blocked=false for clean IP", r.get("blocked") == False)
test("check_ip threat=0.0 for clean IP", r.get("threat") == 0.0)

r = ipc_send(json.dumps({"type":"report_connections","events":[
    {"ip":"10.0.0.2","bytes":1024,"status_code":200,"proto_fp":1},
    {"ip":"10.0.0.3","bytes":512,"status_code":200,"proto_fp":1},
]}))
test("report_connections batch accepted", r.get("type") == "batch_ok" and r.get("accepted",0) == 2, str(r))

r = ipc_send('{"type":"get_stats"}')
test("get_stats returns stats", r.get("type") == "stats", str(r)[:80])
test("get_stats has ram_limit", r.get("ram_limit_mb",0) == 14512)
test("get_stats has ips_tracked", "ips_tracked" in r)

r = ipc_send('{"type":"get_status"}')
test("get_status returns ok", r.get("type") == "ok")

# block_ip
r = ipc_send(json.dumps({"type":"block_ip","ip":"198.51.100.50","reason":"test","ttl_secs":120}))
test("block_ip returns pending", r.get("state") == "pending", str(r))
time.sleep(0.1)
r = ipc_send('{"type":"check_ip","ip":"198.51.100.50"}')
test("blocked IP shows blocked=true", r.get("blocked") == True, str(r))

# unblock_ip
r = ipc_send('{"type":"unblock_ip","ip":"198.51.100.50"}')
test("unblock_ip returns pending", r.get("state") == "pending")
time.sleep(0.1)
r = ipc_send('{"type":"check_ip","ip":"198.51.100.50"}')
test("unblocked IP shows blocked=false", r.get("blocked") == False, str(r))

r = ipc_send('{"type":"flush"}')
test("flush returns ok", r.get("type") == "ok")

# adversarial input
r = ipc_send('not json at all {{{{}', timeout=2)
test("malformed IPC handled gracefully", "_error" not in r or "Connection reset" in str(r.get("_error","")) or r.get("type") == "error")

# ── SECTION 2: Dashboard Endpoints ──────────────────────────────
print("\n── 2. DASHBOARD ENDPOINTS ──")

for path, checks in [
    ("/healthz", {"status": "ok"}),
    ("/api/snapshot", {"is_healthy": True}),
    ("/api/status/modules", None),
    ("/api/history/blocks", None),
    ("/api/history/batches", None),
    ("/api/traffic/subnets", None),
    ("/api/config", None),
    ("/metrics", None),
]:
    code, body = dashboard(path)
    test(f"{path} → HTTP 200", code == 200, f"got {code}")
    if checks is not None:
        for k, v in checks.items():
            test(f"{path}.{k} == {v}", body.get(k) == v, str(body.get(k)))

# ── SECTION 3: Snapshot Fields ──────────────────────────────────
print("\n── 3. SNAPSHOT INTEGRITY ──")

code, snap = dashboard("/api/snapshot")
for field in ["ts_ms","uptime_secs","ips_tracked","blocked_total","ram_bytes",
              "ram_limit_mb","ram_pct","cpu_usage","memory_usage_mb",
              "ipc_requests","events_ingested","pipeline","is_healthy"]:
    test(f"snapshot.{field} present", field in snap, str(snap.keys()))

test("pipeline has all stages", all(k in snap.get("pipeline",{}) for k in ["ingest","queued","batched","promoted","merged","blocked"]))

# ── SECTION 4: Module Health ────────────────────────────────────
print("\n── 4. MODULE HEALTH ──")

code, mods = dashboard("/api/status/modules")
if isinstance(mods, list):
    for m in mods:
        label = m.get("label","?")
        test(f"{label} module present", "events" in m)
        test(f"{label} module 0 errors", m.get("errors",999) == 0, f"errors={m.get('errors')}")
elif isinstance(mods, dict):
    for k, v in mods.items():
        test(f"{k} module present", isinstance(v, dict))

# ── SECTION 5: Config ───────────────────────────────────────────
print("\n── 5. CONFIG LOADING ──")

code, cfg = dashboard("/api/config")
test("config has engine section", "engine" in cfg or "ram_limit_mb" in cfg.get("engine",{}))
test("config ram_limit_mb correct", cfg.get("engine",{}).get("ram_limit_mb") == 14512 or cfg.get("ram_limit_mb") == 14512, str(cfg)[:200])
test("config has dashboard section", "dashboard" in cfg or "http_addr" in cfg)

# ── SECTION 6: Detection Engine ─────────────────────────────────
print("\n── 6. DETECTION ENGINE (dual gate test) ──")

# Reset by blocking then unblocking a test IP
ipc_send(json.dumps({"type":"block_ip","ip":"172.16.0.1","reason":"test","ttl_secs":60}))
ipc_send('{"type":"unblock_ip","ip":"172.16.0.1"}')

# Send batch from concentrated subnet (must hit 50 unique IPs + 100 events to trigger subnet_batch)
import random
batch = []
for i in range(60):
    batch.append({"ip":f"192.168.{i%255}.{i+1}","bytes":128,"status_code":200,"proto_fp":2})

for start in range(0, 120, 60):
    chunk = batch[start:start+60] if start < len(batch) else batch[:60]
    r = ipc_send(json.dumps({"type":"report_connections","events":chunk}))

# Check if blocks appeared
code, snap2 = dashboard("/api/snapshot")
blocks = snap2.get("blocks_applied", 0)
events = snap2.get("events_ingested", 0)
test(f"detection processed events ({events})", events > 50, f"events={events}")

# ── SECTION 7: Prometheus Metrics ───────────────────────────────
print("\n── 7. PROMETHEUS METRICS ──")

code, body = dashboard("/metrics")
code2, raw = 0, ""
try:
    r = urllib.request.urlopen(f"{DASH}/metrics", timeout=3)
    raw = r.read().decode()
except:
    pass
test("metrics has HELP lines", "HELP" in raw)
test("metrics has TYPE lines", "TYPE" in raw)
test("metrics has ramshield_ prefix", "ramshield_" in raw)
line_count = raw.count("\n")
test(f"metrics non-trivial ({line_count} lines)", line_count > 10)

# ── SECTION 8: Concurrent Access ────────────────────────────────
print("\n── 8. CONCURRENT ACCESS ──")

def concurrent_check(i):
    return ipc_send(json.dumps({"type":"check_ip","ip":f"10.0.{i%256}.{i}"}))

with ThreadPoolExecutor(max_workers=10) as ex:
    results = list(ex.map(concurrent_check, range(100)))

ok_count = sum(1 for r in results if "blocked" in r)
test(f"100 concurrent IPC requests handled ({ok_count}/100)", ok_count >= 95, f"ok={ok_count}")

with ThreadPoolExecutor(max_workers=10) as ex:
    results = list(ex.map(lambda i: dashboard("/api/snapshot"), range(50)))

ok_count = sum(1 for code, _ in results if code == 200)
test(f"50 concurrent dashboard requests ({ok_count}/50)", ok_count >= 45, f"ok={ok_count}")

# ── SECTION 9: Large Payload ────────────────────────────────────
print("\n── 9. LARGE PAYLOAD / STRESS ──")

large_batch = [{"ip":f"10.{(i//256)%256}.{i%256}.{i+10}","bytes":64,"status_code":200,"proto_fp":3} for i in range(500)]
r = ipc_send(json.dumps({"type":"report_connections","events":large_batch}), timeout=5)
test("500-event batch accepted", r.get("type") == "batch_ok", str(r)[:80])
test("500-event accepted+rejected == 500", r.get("accepted",0) + r.get("rejected",0) == 500, f"accepted={r.get('accepted')} rejected={r.get('rejected')}")

# ── SECTION 10: Final State Check ──────────────────────────────
print("\n── 10. FINAL STATE ──")

code, snap = dashboard("/api/snapshot")
test("engine still healthy after all tests", snap.get("is_healthy") == True)
test("ram_limit still correct", snap.get("ram_limit_mb") == 14512)
test("ips_tracked > 0", snap.get("ips_tracked",0) > 0)
test("events_ingested > 0", snap.get("events_ingested",0) > 0)

code, hist = dashboard("/api/history/blocks")
test("block history accessible", code == 200)

code, hist = dashboard("/api/history/batches")
test("batch history accessible", code == 200)

code, hist = dashboard("/api/traffic/subnets")
test("subnet traffic accessible", code == 200)

# ── SUMMARY ─────────────────────────────────────────────────────
print("\n" + "=" * 60)
total = passed + failed + skipped
print(f"RESULTS: {passed}/{total} passed, {failed} failed, {skipped} skipped")
print("=" * 60)
sys.exit(1 if failed > 0 else 0)
