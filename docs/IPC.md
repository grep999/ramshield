# RamShield IPC Protocol

Wire format: newline-delimited JSON over TCP. One JSON `Request` per line, one JSON `Response` per line. Server is `src/engine/mod.rs::ipc_server`; framing handled by `conn_handler`.

Transport: TCP, address from `engine.ipc.tcp_addr`, max concurrent connections from `engine.max_connections`.

## Requests (`tag = "type"`, snake_case)

| Type | Fields |
|------|--------|
| `check_ip`           | `ip: string` |
| `block_ip`           | `ip: string`, `reason: string`, `ttl_secs: number \| null` |
| `unblock_ip`         | `ip: string` |
| `get_ip_stats`       | `ip: string` |
| `get_stats`          | — |
| `report_connection`  | `ip: string`, `bytes: number`, `status_code: number`, `proto_fp: number` |
| `report_connections` | `events: [{ ip, bytes, status_code, proto_fp }, ...]` |
| `flush`              | — |

## Responses

| Type | Fields |
|------|--------|
| `ip_status`  | `ip`, `blocked`, `threat`, `ewma_rps`, `reason` |
| `ok`         | `message` |
| `batch_ok`   | `accepted`, `rejected` |
| `error`      | `code` (4xx/5xx), `message` |
| `stats`      | `ips_tracked`, `blocked`, `ram_bytes`, `ram_limit_mb`, `uptime_secs`, `evictions` |
| `ip_detail`  | `ip`, `count`, `ewma_rps`, `threat`, `state`, `bytes_in`, `first_seen_s`, `last_seen_s` |

## Errors

- Invalid JSON → `{"type":"error","code":400,"message":"<serde err>"}`
- Invalid IP → `code:400`
- Unknown IP (for get_ip_stats) → `code:404`
- Detection channel full → `code:503`
- Serialise failure (server-side) → `{"type":"error","code":500,"message":"serialise failed"}`

## Minimal example (POSIX nc)

```sh
printf '{"type":"check_ip","ip":"1.2.3.4"}\n' | nc -w1 127.0.0.1 7900
# {"type":"ip_status","ip":"1.2.3.4","blocked":false,"threat":0.0,"ewma_rps":0.0,"reason":null}
```

## Source of truth

- Types: `src/ipc/mod.rs`
- Server / framing: `src/engine/mod.rs` (`ipc_server`, `conn_handler`)
