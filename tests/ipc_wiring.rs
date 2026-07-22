use ramshield::{Config, Engine};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn ipc_wiring_binds_and_accepts() {
    let mut cfg = Config::default();
    cfg.ipc.tcp_addr = "127.0.0.1:17890".into(); // off default port — don't collide

    let store = Arc::new(ramshield::storage::Store::new(cfg.engine.shard_count));
    let metrics = Arc::new(ramshield::metrics::Metrics::new());
    let engine = Arc::new(Engine::new(cfg.clone(), store, metrics));
    let _handle = engine.clone().start_async().expect("start");

    // Give the OS thread time to bind
    std::thread::sleep(Duration::from_secs(1));

    let mut stream = TcpStream::connect("127.0.0.1:17890")
        .expect("TCP connect to IPC server");

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