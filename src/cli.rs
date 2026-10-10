use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

const MAX_IPC_RESPONSE_BYTES: usize = 1024 * 1024;
const IPC_IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Decode a CLI HMAC key using the same minimum size enforced by server config.
fn decode_hmac_key(hex_key: &str) -> Result<Vec<u8>> {
    let key = hex::decode(hex_key.trim()).map_err(|e| anyhow::anyhow!("bad key hex: {e}"))?;
    if key.len() < 16 {
        anyhow::bail!("IPC HMAC key must be at least 16 bytes (32 hex characters)");
    }
    Ok(key)
}

/// Read exactly one newline-terminated IPC response with a hard size limit.
/// A timeout is configured on the socket by the caller, so a peer cannot hold
/// the CLI indefinitely while sending an incomplete frame.
fn read_response_line<R: BufRead>(reader: &mut R) -> Result<String> {
    let mut response = Vec::with_capacity(4096);
    loop {
        let available = reader.fill_buf().context("reading IPC response")?;
        if available.is_empty() {
            break;
        }

        let remaining = MAX_IPC_RESPONSE_BYTES.saturating_sub(response.len());
        let inspect_len = available.len().min(remaining);
        if let Some(newline) = available[..inspect_len]
            .iter()
            .position(|byte| *byte == b'\n')
        {
            response.extend_from_slice(&available[..=newline]);
            reader.consume(newline + 1);
            return String::from_utf8(response).context("IPC response is not valid UTF-8");
        }
        if available.len() >= remaining {
            anyhow::bail!(
                "IPC response exceeds {MAX_IPC_RESPONSE_BYTES} bytes or is not newline-terminated"
            );
        }

        let consumed = available.len();
        response.extend_from_slice(available);
        reader.consume(consumed);
    }

    anyhow::bail!("IPC server closed before sending a newline-terminated response")
}

#[derive(Parser)]
#[command(name = "ramshield-cli", about = "RamShield CLI")]
struct Cli {
    #[arg(short, long, default_value = "127.0.0.1:7890")]
    addr: String,
    /// Shared HMAC key (hex). Read from RAMSHIELD_IPC_KEY when servers
    /// require auth. Must be at least 16 bytes (32 hex characters).
    /// Omitted = unsigned frames (open servers).
    #[arg(long)]
    key: Option<String>,
    /// Key identifier matching an entry in the server's ipc.auth_keys.
    #[arg(long, default_value = "k1")]
    key_id: String,
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
    UnblockCidr {
        cidr: String,
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
        Cmd::UnblockCidr { cidr } => {
            serde_json::json!({"type": "unblock_cidr", "cidr": cidr}).to_string()
        }
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
        let mut v: Value = serde_json::from_str(&json).context("built frame must be valid JSON")?;
        let ts_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .context("system clock is before UNIX epoch")?
            .as_millis() as u64;
        let payload = serde_json::to_vec(&v)?;
        let key = decode_hmac_key(hexkey)?;
        let key_id = cli.key_id.trim();
        if key_id.is_empty() {
            anyhow::bail!("IPC key id must not be empty");
        }

        let mut mac = Hmac::<Sha256>::new_from_slice(&key)
            .map_err(|e| anyhow::anyhow!("hmac init: {}", e))?;
        mac.update(ts_ms.to_string().as_bytes());
        mac.update(b".");
        mac.update(key_id.as_bytes());
        mac.update(&payload);
        let sig = hex::encode(mac.finalize().into_bytes());
        let obj = v
            .as_object_mut()
            .context("auth frame root must be a JSON object")?;
        obj.insert(
            "auth".into(),
            serde_json::json!({
                "key_id": key_id, "ts_ms": ts_ms, "sig": sig
            }),
        );
        v.to_string()
    } else {
        json
    };

    let mut stream = TcpStream::connect(&cli.addr)
        .map_err(|e| anyhow::anyhow!("cannot connect to {}: {}", cli.addr, e))?;
    stream
        .set_read_timeout(Some(IPC_IO_TIMEOUT))
        .context("setting IPC read timeout")?;
    stream
        .set_write_timeout(Some(IPC_IO_TIMEOUT))
        .context("setting IPC write timeout")?;
    writeln!(stream, "{}", json)?;

    let resp = read_response_line(&mut BufReader::new(&stream))?;
    let v: serde_json::Value =
        serde_json::from_str(&resp).unwrap_or(serde_json::Value::String(resp.trim().into()));
    if compact {
        println!("{}", serde_json::to_string(&v)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&v)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MAX_IPC_RESPONSE_BYTES, read_response_line};
    use std::io::Cursor;

    #[test]
    fn hmac_key_matches_server_minimum_length() {
        assert!(super::decode_hmac_key("00112233445566778899aabbccddeeff").is_ok());
        let err = super::decode_hmac_key("0011223344556677").unwrap_err();
        assert!(err.to_string().contains("at least 16 bytes"));
    }

    #[test]
    fn hmac_key_rejects_invalid_hex() {
        assert!(super::decode_hmac_key("zz112233445566778899aabbccddeeff").is_err());
    }

    #[test]
    fn response_reader_accepts_a_single_newline_terminated_frame() {
        let mut input = Cursor::new(b"{\"ok\":true}\ntrailing".to_vec());
        assert_eq!(read_response_line(&mut input).unwrap(), "{\"ok\":true}\n");
    }

    #[test]
    fn response_reader_rejects_oversized_frames() {
        let mut input = Cursor::new(vec![b'x'; MAX_IPC_RESPONSE_BYTES + 1]);
        assert!(read_response_line(&mut input).is_err());
    }

    #[test]
    fn response_reader_rejects_unterminated_eof() {
        let mut input = Cursor::new(b"partial frame".to_vec());
        assert!(read_response_line(&mut input).is_err());
    }
}
