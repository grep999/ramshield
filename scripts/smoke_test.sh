#!/bin/bash
set -euo pipefail
PORT=7890
DASH="http://127.0.0.1:9999"

echo "=== IPC MODULE TESTS ==="

echo -n "check_ip:        "
R=$(echo '{"type":"check_ip","ip":"198.51.100.5"}' | nc -q2 -w3 127.0.0.1 $PORT)
echo "$R" | python3 -c "import sys,json;d=json.load(sys.stdin);print('OK' if 'blocked' in d else 'FAIL')"

echo -n "report_conns:    "
R=$(echo '{"type":"report_connections","events":[{"ip":"10.0.0.1","bytes":128,"status_code":200,"proto_fp":2}]}' | nc -q2 -w3 127.0.0.1 $PORT)
echo "$R" | python3 -c "import sys,json;d=json.load(sys.stdin);print('OK' if d.get('type')=='batch_ok' else f'FAIL {d}')"

echo -n "get_stats:       "
R=$(echo '{"type":"get_stats"}' | nc -q2 -w3 127.0.0.1 $PORT)
echo "$R" | python3 -c "import sys,json;d=json.load(sys.stdin);print(f'OK tracked={d[\"ips_tracked\"]} blocked={d[\"blocked\"]} limit={d[\"ram_limit_mb\"]}MB')"

echo -n "get_status:      "
R=$(echo '{"type":"get_status"}' | nc -q2 -w3 127.0.0.1 $PORT)
echo "$R" | python3 -c "import sys,json;d=json.load(sys.stdin);print(f'OK healthy={d.get(\"is_healthy\",d.get(\"healthy\",\"?\"))}')"

echo -n "block_ip:        "
R=$(echo '{"type":"block_ip","ip":"203.0.113.50","reason":"smoke","ttl_secs":60}' | nc -q2 -w3 127.0.0.1 $PORT)
echo "$R" | python3 -c "import sys,json;d=json.load(sys.stdin);print(f'OK state={d.get(\"state\",\"?\")}')"

echo -n "check_blocked:   "
R=$(echo '{"type":"check_ip","ip":"203.0.113.50"}' | nc -q2 -w3 127.0.0.1 $PORT)
echo "$R" | python3 -c "import sys,json;d=json.load(sys.stdin);print(f'OK blocked={d[\"blocked\"]}')"

echo -n "unblock_ip:      "
R=$(echo '{"type":"unblock_ip","ip":"203.0.113.50"}' | nc -q2 -w3 127.0.0.1 $PORT)
echo "$R" | python3 -c "import sys,json;d=json.load(sys.stdin);print(f'OK state={d.get(\"state\",\"?\")}')"

echo -n "flush:           "
R=$(echo '{"type":"flush"}' | nc -q2 -w3 127.0.0.1 $PORT)
echo "$R" | python3 -c "import sys,json;d=json.load(sys.stdin);print(f'OK count={d.get(\"count\",0)}')"

echo ""
echo "=== DASHBOARD ENDPOINTS ==="
for ep in /healthz /api/snapshot /api/status/modules /api/history/blocks /api/history/batches /api/traffic/subnets /api/config /metrics; do
    echo -n "$ep:  "
    HTTP=$(curl -s -m 3 -o /dev/null -w "%{http_code}" "$DASH$ep")
    SIZE=$(curl -s -m 3 "$DASH$ep" | wc -c)
    if [ "$HTTP" = "200" ]; then
        echo "OK ($HTTP, ${SIZE}B)"
    else
        echo "FAIL ($HTTP)"
    fi
done

echo ""
echo "=== SNAPSHOT DETAIL ==="
curl -s -m 3 $DASH/api/snapshot | python3 -c "
import sys,json
d=json.load(sys.stdin)
print(f'  healthy:    {d[\"is_healthy\"]}')
print(f'  events:     {d[\"events_ingested\"]}')
print(f'  batches:    {d[\"batches_total\"]}')
print(f'  ips:        {d[\"ips_tracked\"]}')
print(f'  blocks:     {d[\"blocks_applied\"]}')
print(f'  ram_limit:  {d[\"ram_limit_mb\"]}MB')
print(f'  cpu:        {d[\"cpu_usage\"]:.1f}%')
print(f'  memory:     {d[\"memory_usage_mb\"]}MB')
"

echo ""
echo "=== MODULE STATUS ==="
curl -s -m 3 $DASH/api/status/modules | python3 -c "
import sys,json
d=json.load(sys.stdin)
for k,v in sorted(d.items()):
    if isinstance(v,dict):
        ok = v.get('is_healthy', v.get('healthy', None))
        errs = v.get('errors', v.get('error',0))
        print(f'  {k:<16} healthy={ok}  errors={errs}')
    elif isinstance(v,list):
        print(f'  {k:<16} [{len(v)} entries]')
    else:
        print(f'  {k:<16} {v}')
"
