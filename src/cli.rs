use anyhow::Result;
use clap::{Parser, Subcommand};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

#[derive(Parser)]
#[command(name = "ramshield-cli", about = "RamShield CLI")]
struct Cli {
    #[arg(short, long, default_value = "127.0.0.1:7890")]
    addr: String,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Check   { ip: String },
    Block   {
        ip: String,
        #[arg(short, long, default_value = "manual")] reason: String,
        #[arg(short, long)] ttl: Option<u64>,
    },
    Unblock { ip: String },
    Stats,
    Status,
    Info    { ip: String },
}

fn main() -> Result<()> {
    let cli  = Cli::parse();
    let json = match &cli.cmd {
        Cmd::Check   { ip }              => format!(r#"{{"type":"check_ip","ip":"{}"}}"#, ip),
        Cmd::Block   { ip, reason, ttl } => match ttl {
            Some(t) => format!(r#"{{"type":"block_ip","ip":"{}","reason":"{}","ttl_secs":{}}}"#, ip, reason, t),
            None    => format!(r#"{{"type":"block_ip","ip":"{}","reason":"{}","ttl_secs":null}}"#, ip, reason),
        },
        Cmd::Unblock { ip } => format!(r#"{{"type":"unblock_ip","ip":"{}"}}"#, ip),
        Cmd::Stats => r#"{"type":"get_stats"}"#.into(),
        Cmd::Status => r#"{"type":"get_status"}"#.into(),
        Cmd::Info { ip } => format!(r#"{{"type":"get_ip_stats","ip":"{}"}}"#, ip),
    };

    let mut stream = TcpStream::connect(&cli.addr)
        .map_err(|e| anyhow::anyhow!("cannot connect to {}: {}", cli.addr, e))?;
    writeln!(stream, "{}", json)?;

    let mut resp = String::new();
    BufReader::new(&stream).read_line(&mut resp)?;
    let v: serde_json::Value = serde_json::from_str(&resp)
        .unwrap_or(serde_json::Value::String(resp.trim().into()));
    println!("{}", serde_json::to_string_pretty(&v)?);
    Ok(())
}
