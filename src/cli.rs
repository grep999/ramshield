use anyhow::Result;
use clap::{Parser, Subcommand};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

#[derive(Parser)]
#[command(name = "ramshield-cli", about = "RamShield CLI")]
struct Cli {
    #[arg(short, long, default_value = "127.0.0.1:7890")]
    addr: String,
    /// Shared HMAC key (hex). Read from RAMSHIELD_IPC_KEY when servers
    /// require auth. Omitted = unsigned frames (open servers).
    #[arg(long)]
    key: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Check {
        ip: String,
    },
    Block {
        ip: String,
        #[arg(short, long, default_value = "manual")]
        reason: String,
        #[arg(short, long)]
        ttl: Option<u64>,
    },
    Unblock {
        ip: String,
    },
    Stats,
    Status {
        #[arg(long)]
        json: bool,
    },
    Info {
        ip: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let json = match &cli.cmd {
        Cmd::Check { ip } => serde_json::json!({"type": "check_ip", "ip": ip}).to_string(),
        Cmd::Block { ip, reason, ttl } => {
            serde_json::json!({"type": "block_ip", "ip": ip, "reason": reason, "ttl_secs": ttl})
                .to_string()
        }
        Cmd::Unblock { ip } => serde_json::json!({"type": "unblock_ip", "ip": ip}).to_string(),
        Cmd::Stats => r#"{"type":"get_stats"}"#.into(),
        Cmd::Status { .. } => r#"{"type":"get_status"}"#.into(),
        Cmd::Info { ip } => serde_json::json!({"type": "get_ip_stats", "ip": ip}).to_string(),
    };

    let compact = matches!(&cli.cmd, Cmd::Status { json: true });

    // Auth: RAMSHIELD_IPC_KEY (hex) or --key. When set, wrap the frame:
    // {"auth":{"key_id","ts_ms","sig"},"type":...} where sig = HMAC-SHA256
    // over "<ts_ms>.<compact frame json without auth>".
    let key_hex = cli.key.or_else(|| std::env::var("RAMSHIELD_IPC_KEY").ok());
    let json = if let Some(hexkey) = &key_hex {
        use serde_json::Value;
        let mut v: Value = serde_json::from_str(&json).expect("built frame must be valid JSON");
        let ts_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let payload = serde_json::to_vec(&v)?;
        let key = hex::decode(hexkey.trim()).map_err(|e| anyhow::anyhow!("bad key hex: {}", e))?;
        let mut mac = Hmac::<Sha256>::new_from_slice(&key)
            .map_err(|e| anyhow::anyhow!("hmac init: {}", e))?;
        mac.update(ts_ms.to_string().as_bytes());
        mac.update(b".");
        mac.update(&payload);
        let sig = hex::encode(mac.finalize().into_bytes());
        v.as_object_mut().unwrap().insert(
            "auth".into(),
            serde_json::json!({
                "key_id": "k1", "ts_ms": ts_ms, "sig": sig
            }),
        );
        v.to_string()
    } else {
        json
    };

    let mut stream = TcpStream::connect(&cli.addr)
        .map_err(|e| anyhow::anyhow!("cannot connect to {}: {}", cli.addr, e))?;
    writeln!(stream, "{}", json)?;

    let mut resp = String::new();
    BufReader::new(&stream).read_line(&mut resp)?;
    let v: serde_json::Value =
        serde_json::from_str(&resp).unwrap_or(serde_json::Value::String(resp.trim().into()));
    if compact {
        println!("{}", serde_json::to_string(&v)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&v)?);
    }
    Ok(())
}
