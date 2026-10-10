use crate::aworset::{AworsetBlocklist, ClusterBlockDelta, ClusterUnblockDelta};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{collections::VecDeque, future::Future, net::SocketAddr, sync::Arc, time::{Duration, SystemTime, UNIX_EPOCH}};
use tokio::{io::{AsyncRead, AsyncReadExt, AsyncWriteExt}, net::{TcpListener, TcpStream}, sync::{Mutex, Semaphore}};
use tracing::{debug, warn};

type HmacSha256 = Hmac<Sha256>;
const MAX_FRAME: usize = 64 * 1024;
const MAX_CLOCK_SKEW_MS: u64 = 30_000;
const ANTI_ENTROPY_MS: u64 = 2_000;
const MAX_SYNC_ENTRIES: usize = 4096;
const MAX_SYNC_FRAME_ENTRIES: usize = 128;
const MAX_INCOMING_QUEUE: usize = 8192;
const MAX_PEER_READERS: usize = 256;
const MAX_FRAME_READ_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PEER_WRITERS: usize = 64;
const MAX_PEER_WRITE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MeshMessage { Block(ClusterBlockDelta), Unblock(ClusterUnblockDelta), Sync { blocks: Vec<ClusterBlockDelta>, unblocks: Vec<ClusterUnblockDelta> } }
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Envelope { ts_ms: u64, node_id: u32, body: MeshMessage, mac: String }
#[derive(Clone)]
pub struct MeshHandle { node_id: u32, blocklist: Arc<AworsetBlocklist>, peers: Arc<Vec<SocketAddr>>, auth_key: Arc<Vec<u8>>, incoming: Arc<Mutex<VecDeque<MeshMessage>>>, readers: Arc<Semaphore>, writers: Arc<Semaphore> }

impl MeshHandle {
    pub async fn bind(node_id: u32, blocklist: Arc<AworsetBlocklist>, listen: SocketAddr, peers: Vec<SocketAddr>, auth_key: Vec<u8>) -> std::io::Result<Self> {
        let listener = TcpListener::bind(listen).await?;
        let handle = Self { node_id, blocklist, peers: Arc::new(peers), auth_key: Arc::new(auth_key), incoming: Arc::new(Mutex::new(VecDeque::new())), readers: Arc::new(Semaphore::new(MAX_PEER_READERS)), writers: Arc::new(Semaphore::new(MAX_PEER_WRITERS)) };
        let accept_handle = handle.clone();
        tokio::spawn(async move {
            loop { match listener.accept().await { Ok((stream, _)) => {
                    let h=accept_handle.clone();
                    let Ok(permit) = h.readers.clone().try_acquire_owned() else { continue; };
                    tokio::spawn(async move { h.read_stream(stream).await; drop(permit); });
                }, Err(e) => { warn!(error=%e, "mesh listener stopped"); break; } } }
        });
        let sync_handle = handle.clone();
        tokio::spawn(async move { let mut tick=tokio::time::interval(Duration::from_millis(ANTI_ENTROPY_MS)); loop { tick.tick().await; sync_handle.broadcast_sync().await; } });
        Ok(handle)
    }
    /// Send to configured peers with a strict cap on concurrent outbound tasks.
    /// The permit is acquired before spawning, so there is no unbounded waiter/task queue.
    pub async fn broadcast(&self, body: MeshMessage) {
        for peer in self.peers.iter().copied() {
            let permit = match self.writers.clone().acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => {
                    warn!(peer = %peer, "mesh outbound semaphore closed; peer send skipped");
                    continue;
                }
            };
            let handle = self.clone();
            let message = body.clone();
            tokio::spawn(async move {
                let _permit = permit;
                match with_timeout(MAX_PEER_WRITE_TIMEOUT, handle.send_to(peer, message)).await {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::TimedOut => {
                        warn!(peer = %peer, "mesh outbound send timed out");
                    }
                    Err(error) => debug!(peer = %peer, error = %error, "mesh send failed"),
                }
            });
        }
    }
    pub async fn drain(&self) -> Vec<MeshMessage> { let mut q=self.incoming.lock().await; q.drain(..).collect() }
    async fn broadcast_sync(&self) {
        let (blocks, unblocks) = self.blocklist.snapshot(MAX_SYNC_ENTRIES);
        for message in chunk_sync(&blocks, &unblocks) {
            self.broadcast(message).await;
        }
    }
    async fn send_to(&self, peer: SocketAddr, body: MeshMessage) -> std::io::Result<()> { let ts_ms=now_ms(); let payload=serde_json::to_vec(&(ts_ms,self.node_id,&body)).map_err(std::io::Error::other)?; let mac=sign(&self.auth_key,&payload)?; let frame=serde_json::to_vec(&Envelope{ts_ms,node_id:self.node_id,body,mac}).map_err(std::io::Error::other)?; if frame.len()>MAX_FRAME { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"mesh frame too large")); } let mut stream=TcpStream::connect(peer).await?; stream.write_all(&frame).await?; stream.write_all(b"\n").await?; Ok(()) }
    async fn read_stream(&self, mut stream: TcpStream) { let mut line=match read_bounded_frame_with_timeout(&mut stream, MAX_FRAME_READ_TIMEOUT).await { Ok(Some(frame))=>frame, Ok(None)|Err(_)=>return }; while line.last().is_some_and(|b| *b==b'\n'||*b==b'\r') { line.pop(); } let env:Envelope=match serde_json::from_slice(&line){Ok(v)=>v,Err(_)=>return}; let payload=match serde_json::to_vec(&(env.ts_ms,env.node_id,&env.body)){Ok(v)=>v,Err(_)=>return}; if now_ms().abs_diff(env.ts_ms)>MAX_CLOCK_SKEW_MS || !verify(&self.auth_key,&payload,&env.mac){ warn!(node_id=env.node_id,"mesh frame rejected: authentication or timestamp invalid"); return; }
        if env.node_id==self.node_id{return;} let mut q = self.incoming.lock().await;
        if q.len() >= MAX_INCOMING_QUEUE {
            // Prefer the newest authenticated state; anti-entropy will recover
            // an older delta if it was displaced. Never allow mesh traffic to
            // consume unbounded memory under a peer flood.
            q.pop_front();
        }
        q.push_back(env.body); }
}
/// Split anti-entropy state into small independently authenticated frames.
fn chunk_sync(
    blocks: &[ClusterBlockDelta],
    unblocks: &[ClusterUnblockDelta],
) -> Vec<MeshMessage> {
    let block_chunks = blocks.len().div_ceil(MAX_SYNC_FRAME_ENTRIES);
    let unblock_chunks = unblocks.len().div_ceil(MAX_SYNC_FRAME_ENTRIES);
    let mut messages = Vec::with_capacity(block_chunks + unblock_chunks);

    messages.extend(blocks.chunks(MAX_SYNC_FRAME_ENTRIES).map(|chunk| {
        MeshMessage::Sync {
            blocks: chunk.to_vec(),
            unblocks: Vec::new(),
        }
    }));
    messages.extend(unblocks.chunks(MAX_SYNC_FRAME_ENTRIES).map(|chunk| {
        MeshMessage::Sync {
            blocks: Vec::new(),
            unblocks: chunk.to_vec(),
        }
    }));
    messages
}

async fn with_timeout<F>(timeout: Duration, future: F) -> std::io::Result<()>
where
    F: Future<Output = std::io::Result<()>>,
{
    tokio::time::timeout(timeout, future)
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "mesh outbound send timed out"))?
}

fn push_frame_bytes(line: &mut Vec<u8>, bytes: &[u8]) -> std::io::Result<bool> {
    if let Some(end) = bytes.iter().position(|byte| *byte == b'\n') {
        if line.len() + end > MAX_FRAME {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "mesh frame too large"));
        }
        line.extend_from_slice(&bytes[..=end]);
        return Ok(true);
    }
    if line.len() + bytes.len() > MAX_FRAME {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "mesh frame too large"));
    }
    line.extend_from_slice(bytes);
    Ok(false)
}

async fn read_bounded_frame_with_timeout<R: AsyncRead + Unpin>(
    reader: &mut R,
    timeout: Duration,
) -> std::io::Result<Option<Vec<u8>>> {
    tokio::time::timeout(timeout, read_bounded_frame(reader))
        .await
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "mesh frame read timed out"))?
}

async fn read_bounded_frame<R: AsyncRead + Unpin>(reader: &mut R) -> std::io::Result<Option<Vec<u8>>> {
    let mut line = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            if line.is_empty() {
                return Ok(None);
            }
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "incomplete mesh frame"));
        }
        if push_frame_bytes(&mut line, &chunk[..read])? {
            return Ok(Some(line));
        }
    }
}

fn sign(key:&[u8],payload:&[u8])->std::io::Result<String>{let mut mac=HmacSha256::new_from_slice(key).map_err(|_|std::io::Error::new(std::io::ErrorKind::InvalidInput,"invalid mesh key"))?;mac.update(payload);Ok(hex::encode(mac.finalize().into_bytes()))}
fn verify(key:&[u8],payload:&[u8],signature:&str)->bool{let bytes=match hex::decode(signature){Ok(v)=>v,Err(_)=>return false};let mut mac=match HmacSha256::new_from_slice(key){Ok(v)=>v,Err(_)=>return false};mac.update(payload);mac.verify_slice(&bytes).is_ok()}
fn now_ms()->u64{SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64}

#[cfg(test)]
mod frame_tests {
    use super::{chunk_sync, push_frame_bytes, read_bounded_frame_with_timeout, with_timeout, Envelope, MeshMessage, MAX_FRAME, MAX_SYNC_FRAME_ENTRIES};
    use crate::aworset::{ClusterBlockDelta, ClusterDot, ClusterUnblockDelta};
    use std::net::{IpAddr, Ipv6Addr};
    use std::time::Duration;

    #[test]
    fn incomplete_peer_frame_times_out() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let (_writer, mut reader) = tokio::io::duplex(8);
            let error = read_bounded_frame_with_timeout(&mut reader, Duration::from_millis(10))
                .await
                .unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        });
    }

    #[test]
    fn sync_chunking_preserves_all_deltas_and_bounds_each_message() {
        let blocks: Vec<_> = (0..300u128)
            .map(|ip| ClusterBlockDelta {
                ip: IpAddr::V6(Ipv6Addr::from(ip)),
                dot: ClusterDot { node_id: 7, counter: ip as u32 },
                created_at_ms: u64::MAX,
                expires_at_ms: u64::MAX,
                tier: 3,
            })
            .collect();
        let unblocks: Vec<_> = (300..570u128)
            .map(|ip| ClusterUnblockDelta {
                ip: IpAddr::V6(Ipv6Addr::from(ip)),
                dot: ClusterDot { node_id: 8, counter: ip as u32 },
            })
            .collect();

        let messages = chunk_sync(&blocks, &unblocks);
        let mut block_count = 0;
        let mut unblock_count = 0;
        for message in messages {
            if let MeshMessage::Sync { blocks, unblocks } = message {
                assert!(blocks.len() + unblocks.len() <= MAX_SYNC_FRAME_ENTRIES);
                block_count += blocks.len();
                unblock_count += unblocks.len();
            } else {
                panic!("chunk_sync must emit only Sync messages");
            }
        }
        assert_eq!(block_count, 300);
        assert_eq!(unblock_count, 270);
    }

    #[test]
    fn maximum_sync_chunk_fits_authenticated_wire_frame() {
        let blocks: Vec<_> = (0..MAX_SYNC_FRAME_ENTRIES)
            .map(|_| ClusterBlockDelta {
                ip: IpAddr::V6(Ipv6Addr::from(u128::MAX)),
                dot: ClusterDot { node_id: u32::MAX, counter: u32::MAX },
                created_at_ms: u64::MAX,
                expires_at_ms: u64::MAX,
                tier: u8::MAX,
            })
            .collect();
        let message = MeshMessage::Sync { blocks, unblocks: Vec::new() };
        let envelope = Envelope {
            ts_ms: u64::MAX,
            node_id: u32::MAX,
            body: message,
            mac: "0".repeat(64),
        };
        let encoded = serde_json::to_vec(&envelope).expect("serialize worst-case sync frame");
        assert!(encoded.len() <= MAX_FRAME, "encoded frame is {} bytes", encoded.len());
    }

    #[test]
    fn outbound_send_deadline_releases_stalled_send() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("test runtime");
        runtime.block_on(async {
            let error = with_timeout(Duration::from_millis(10), std::future::pending())
                .await
                .unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        });
    }

    #[test]
    fn accepts_frame_at_limit_with_delimiter() {
        let mut frame = Vec::new();
        assert!(!push_frame_bytes(&mut frame, &vec![b'x'; MAX_FRAME]).unwrap());
        assert!(push_frame_bytes(&mut frame, b"\n").unwrap());
        assert_eq!(frame.len(), MAX_FRAME + 1);
        assert_eq!(frame.last(), Some(&b'\n'));
    }

    #[test]
    fn rejects_oversized_frame_before_appending_more_bytes() {
        let mut frame = Vec::new();
        assert!(!push_frame_bytes(&mut frame, &vec![b'x'; MAX_FRAME]).unwrap());
        let error = push_frame_bytes(&mut frame, b"x").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(frame.len(), MAX_FRAME);
    }

    #[test]
    fn rejects_oversized_frame_when_delimiter_arrives_late() {
        let mut frame = vec![b'x'; MAX_FRAME];
        let error = push_frame_bytes(&mut frame, b"x\n").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(frame.len(), MAX_FRAME);
    }
}
