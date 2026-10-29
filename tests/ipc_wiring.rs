use ramshield::{Config, Engine};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

fn ephemeral_addr() -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port for test");
    format!("127.0.0.1:{}", l.local_addr().unwrap().port())
}

#[test]
fn ipc_wiring_binds_and_accepts() {
    let mut cfg = Config::default();
    let ipc_addr = ephemeral_addr();
    cfg.ipc.tcp_addr = ipc_addr.clone();

    let store = Arc::new(ramshield::storage::Store::new(cfg.engine.shard_count));
    let metrics = Arc::new(ramshield::metrics::Metrics::new());
    let engine = Arc::new(Engine::new(cfg.clone(), store, metrics));
    let _handle = engine.clone().start_async().expect("start");

    // Give the OS thread time to bind
    std::thread::sleep(Duration::from_secs(1));

    let mut stream = TcpStream::connect(&ipc_addr).expect("TCP connect to IPC server");

    writeln!(stream, r#"{{"type":"get_status"}}"#).expect("write");
    let mut resp = String::new();
    BufReader::new(&stream).read_line(&mut resp).expect("read");

    // Should get valid JSON response
    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
    assert_eq!(v["type"], "ok");
    assert_eq!(v["message"], "ok");

    engine.shutdown();
}

#[test]
fn ipc_wiring_rejects_invalid_json() {
    let mut cfg = Config::default();
    cfg.ipc.tcp_addr = "127.0.0.1:17891".into();

    let store = Arc::new(ramshield::storage::Store::new(cfg.engine.shard_count));
    let metrics = Arc::new(ramshield::metrics::Metrics::new());
    let engine = Arc::new(Engine::new(cfg.clone(), store, metrics));
    let _handle = engine.clone().start_async().expect("start");
    std::thread::sleep(Duration::from_secs(1));

    let mut stream = TcpStream::connect("127.0.0.1:17891").expect("connect");
    writeln!(stream, "not json").expect("write");
    let mut resp = String::new();
    BufReader::new(&stream).read_line(&mut resp).expect("read");

    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
    assert_eq!(v["type"], "error");
    assert_eq!(v["code"], 1);

    engine.shutdown();
}

#[test]
fn ipc_wiring_oversize_connection_gets_error_frame_not_bare_reset() {
    // F4: a client pushing past max_connection_bytes must receive a typed
    // 413 error frame before the server closes, instead of a bare RST.
    let mut cfg = Config::default();
    cfg.ipc.tcp_addr = "127.0.0.1:17892".into();

    let store = Arc::new(ramshield::storage::Store::new(cfg.engine.shard_count));
    let metrics = Arc::new(ramshield::metrics::Metrics::new());
    let engine = Arc::new(Engine::new(cfg.clone(), store, metrics));
    let _handle = engine.clone().start_async().expect("start");
    std::thread::sleep(Duration::from_secs(1));

    let mut stream = TcpStream::connect("127.0.0.1:17892").expect("connect");
    // Push >1MB (default max_connection_bytes) with no newline — cap trips first.
    let chunk = vec![b'a'; 65536];
    for _ in 0..20 {
        if stream.write_all(&chunk).is_err() {
            break; // server may close mid-send after sending the error frame
        }
        let _ = stream.flush();
    }

    // The error frame is already in flight when the server trips the cap, so
    // this only waits for it to be scheduled and delivered. Under load a
    // loaded host can take longer than a few seconds to run the server task;
    // a tight timeout here fails the run without saying anything about the
    // behaviour under test.
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    let mut resp = String::new();
    // Error frame should already be in flight / buffered before close.
    match BufReader::new(&stream).read_line(&mut resp) {
        Ok(0) => panic!("server closed without sending error frame"),
        Ok(_) => {
            let v: serde_json::Value =
                serde_json::from_str(&resp).expect("error frame is valid JSON");
            assert_eq!(v["type"], "error");
            assert_eq!(v["code"], 413);
            assert!(
                v["message"]
                    .as_str()
                    .unwrap()
                    .contains("max_connection_bytes")
            );
        }
        Err(e) => panic!("read failed before any frame arrived: {e}"),
    }

    engine.shutdown();
}

/// An unterminated line must not be allowed to buffer past the configured
/// line limit when that limit is the tighter of the two bounds.
///
/// By default `max_line_length` equals `max_connection_bytes`, and the
/// connection byte budget is then the binding constraint (see
/// `ipc_wiring_oversize_connection_gets_error_frame_not_bare_reset`). When an
/// operator configures a line limit BELOW the byte budget, the line limit
/// becomes the binding constraint, and an unterminated line must be stopped
/// at the line limit rather than being allowed to grow to the byte budget.
///
/// The check must look at the untermined TAIL only: the buffer can hold
/// several complete pipelined frames, so its total length is not the length
/// of any one line.
#[test]
fn ipc_wiring_unterminated_line_is_capped_at_the_tighter_line_limit() {
    let mut cfg = Config::default();
    cfg.ipc.tcp_addr = "127.0.0.1:17893".into();
    // The line limit is TIGHTER than the byte budget here.
    cfg.ipc.max_line_length = Some(4096);
    cfg.ipc.max_connection_bytes = Some(1024 * 1024);

    let store = Arc::new(ramshield::storage::Store::new(cfg.engine.shard_count));
    let metrics = Arc::new(ramshield::metrics::Metrics::new());
    let engine = Arc::new(Engine::new(cfg.clone(), store, metrics));
    let _handle = engine.clone().start_async().expect("start");
    std::thread::sleep(Duration::from_secs(1));

    let mut stream = TcpStream::connect("127.0.0.1:17893").expect("connect");
    // A long unterminated line: no newline is ever sent.
    let chunk = vec![b'a'; 8192];
    for _ in 0..4 {
        if stream.write_all(&chunk).is_err() {
            break;
        }
        let _ = stream.flush();
    }

    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    let mut resp = String::new();
    match BufReader::new(&stream).read_line(&mut resp) {
        Ok(0) => panic!("server closed without sending an error frame"),
        Ok(_) => {
            let v: serde_json::Value =
                serde_json::from_str(&resp).expect("error frame is valid JSON");
            assert_eq!(v["type"], "error");
            assert_eq!(v["code"], 413);
            assert!(
                v["message"].as_str().unwrap().contains("max_line_length"),
                "expected the line-limit error, got: {resp}"
            );
        }
        Err(e) => panic!("read failed before any frame arrived: {e}"),
    }

    engine.shutdown();
}

/// P0: an unauthenticated request must not be able to MUTATE enforcement state.
///
/// With the default configuration the IPC has no `auth_keys` and
/// `require_auth = false`, so `auth_enforced` is false and every frame is
/// accepted without an auth envelope. The role table was therefore skipped
/// entirely and the actor fell back to a hard-coded `"admin"` — which meant
/// **any local process could `unblock_ip` and remove an active mitigation** on
/// a customer under attack.
///
/// The read and telemetry paths must stay open: the telemetry ingest is the
/// hot path from the proxy and it carries hostile inbound data, so
/// authenticating it buys nothing.
#[test]
fn ipc_wiring_unauthenticated_mutation_is_refused() {
    let mut cfg = Config::default();
    cfg.ipc.tcp_addr = "127.0.0.1:17895".into();

    let store = Arc::new(ramshield::storage::Store::new(cfg.engine.shard_count));
    let metrics = Arc::new(ramshield::metrics::Metrics::new());
    let engine = Arc::new(Engine::new(cfg.clone(), store, metrics));
    let _handle = engine.clone().start_async().expect("start");
    std::thread::sleep(Duration::from_secs(1));

    let mut stream = TcpStream::connect("127.0.0.1:17895").expect("connect");

    // A state-mutating request with no authenticated principal: refused.
    writeln!(stream, r#"{{"type":"unblock_ip","ip":"203.0.113.7"}}"#).expect("write");
    let mut resp = String::new();
    BufReader::new(&stream).read_line(&mut resp).expect("read");
    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
    assert_eq!(
        v["type"], "error",
        "an unauthenticated mutation must not be accepted: {resp}"
    );
    assert_eq!(
        v["code"], 401,
        "expected an unauthorized error for an unauthenticated mutation, got: {resp}"
    );

    // The telemetry ingest path must stay open — it is the hot path from the
    // proxy and carries hostile inbound data.
    writeln!(
        stream,
        r#"{{"type":"report_connection","ip":"203.0.113.8","bytes":10,"status_code":200,"proto_fp":0}}"#
    )
    .expect("write");
    let mut resp2 = String::new();
    BufReader::new(&stream).read_line(&mut resp2).expect("read");
    let v2: serde_json::Value = serde_json::from_str(&resp2).expect("valid JSON");
    assert_ne!(
        v2["code"], 401,
        "the telemetry ingest path must stay unauthenticated: {resp2}"
    );

    engine.shutdown();
}

/// The operator escape hatch: `ipc.require_auth_for_mutations = false`
/// restores the unauthenticated operator path for deployments that have no
/// credentials to configure.
///
/// The flag is only reachable in the no-credentials configuration — with
/// `auth_keys` set, every frame must carry a valid HMAC envelope and carries
/// a principal, so the gate never fires.
#[test]
fn ipc_wiring_unauthenticated_mutation_is_allowed_when_explicitly_enabled() {
    let mut cfg = Config::default();
    cfg.ipc.tcp_addr = "127.0.0.1:17896".into();
    cfg.ipc.require_auth_for_mutations = false;

    let store = Arc::new(ramshield::storage::Store::new(cfg.engine.shard_count));
    let metrics = Arc::new(ramshield::metrics::Metrics::new());
    let engine = Arc::new(Engine::new(cfg.clone(), store, metrics));
    let _handle = engine.clone().start_async().expect("start");
    std::thread::sleep(Duration::from_secs(1));

    let mut stream = TcpStream::connect("127.0.0.1:17896").expect("connect");
    writeln!(stream, r#"{{"type":"unblock_ip","ip":"203.0.113.7"}}"#).expect("write");
    let mut resp = String::new();
    BufReader::new(&stream).read_line(&mut resp).expect("read");
    let v: serde_json::Value = serde_json::from_str(&resp).expect("valid JSON");
    assert_ne!(
        v["code"], 401,
        "the escape hatch must let the request past the gate, got: {resp}"
    );

    engine.shutdown();
}
