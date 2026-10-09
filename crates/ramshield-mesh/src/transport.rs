use crate::aworset::{AworsetBlocklist, ClusterBlockDelta, ClusterUnblockDelta};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{collections::VecDeque, net::SocketAddr, sync::Arc, time::{Duration, SystemTime, UNIX_EPOCH}};
use tokio::{io::{AsyncRead, AsyncReadExt, AsyncWriteExt}, net::{TcpListener, TcpStream}, sync::{Mutex, Semaphore}};
use tracing::{debug, warn};

type HmacSha256 = Hmac<Sha256>;
const MAX_FRAME: usize = 64 * 1024;
const MAX_CLOCK_SKEW_MS: u64 = 30_000;
const ANTI_ENTROPY_MS: u64 = 2_000;
const MAX_SYNC_ENTRIES: usize = 4096;
const MAX_INCOMING_QUEUE: usize = 8192;
const MAX_PEER_READERS: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MeshMessage { Block(ClusterBlockDelta), Unblock(ClusterUnblockDelta), Sync { blocks: Vec<ClusterBlockDelta>, unblocks: Vec<ClusterUnblockDelta> } }
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Envelope { ts_ms: u64, node_id: u32, body: MeshMessage, mac: String }
#[derive(Clone)]
pub struct MeshHandle { node_id: u32, blocklist: Arc<AworsetBlocklist>, peers: Arc<Vec<SocketAddr>>, auth_key: Arc<Vec<u8>>, incoming: Arc<Mutex<VecDeque<MeshMessage>>>, readers: Arc<Semaphore> }

impl MeshHandle {
    pub async fn bind(node_id: u32, blocklist: Arc<AworsetBlocklist>, listen: SocketAddr, peers: Vec<SocketAddr>, auth_key: Vec<u8>) -> std::io::Result<Self> {
        let listener = TcpListener::bind(listen).await?;
        let handle = Self { node_id, blocklist, peers: Arc::new(peers), auth_key: Arc::new(auth_key), incoming: Arc::new(Mutex::new(VecDeque::new())), readers: Arc::new(Semaphore::new(MAX_PEER_READERS)) };
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
    pub async fn broadcast(&self, body: MeshMessage) { for peer in self.peers.iter().copied() { let h=self.clone(); let msg=body.clone(); tokio::spawn(async move { if let Err(e)=h.send_to(peer,msg).await { debug!(peer=%peer,error=%e,"mesh send failed"); } }); } }
    pub async fn drain(&self) -> Vec<MeshMessage> { let mut q=self.incoming.lock().await; q.drain(..).collect() }
    async fn broadcast_sync(&self) { let (blocks,unblocks)=self.blocklist.snapshot(MAX_SYNC_ENTRIES); if blocks.is_empty() && unblocks.is_empty() { return; } self.broadcast(MeshMessage::Sync{blocks,unblocks}).await; }
    async fn send_to(&self, peer: SocketAddr, body: MeshMessage) -> std::io::Result<()> { let ts_ms=now_ms(); let payload=serde_json::to_vec(&(ts_ms,self.node_id,&body)).map_err(std::io::Error::other)?; let mac=sign(&self.auth_key,&payload)?; let frame=serde_json::to_vec(&Envelope{ts_ms,node_id:self.node_id,body,mac}).map_err(std::io::Error::other)?; if frame.len()>MAX_FRAME { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData,"mesh frame too large")); } let mut stream=TcpStream::connect(peer).await?; stream.write_all(&frame).await?; stream.write_all(b"\n").await?; Ok(()) }
    async fn read_stream(&self, mut stream: TcpStream) { let mut line=match read_bounded_frame(&mut stream).await { Ok(Some(frame))=>frame, Ok(None)|Err(_)=>return }; while line.last().is_some_and(|b| *b==b'\n'||*b==b'\r') { line.pop(); } let env:Envelope=match serde_json::from_slice(&line){Ok(v)=>v,Err(_)=>return}; let payload=match serde_json::to_vec(&(env.ts_ms,env.node_id,&env.body)){Ok(v)=>v,Err(_)=>return}; if now_ms().abs_diff(env.ts_ms)>MAX_CLOCK_SKEW_MS || !verify(&self.auth_key,&payload,&env.mac){ warn!(node_id=env.node_id,"mesh frame rejected: authentication or timestamp invalid"); return; } if env.node_id==self.node_id{return;} let mut q = self.incoming.lock().await;
        if q.len() >= MAX_INCOMING_QUEUE {
            // Prefer the newest authenticated state; anti-entropy will recover
            // an older delta if it was displaced. Never allow mesh traffic to
            // consume unbounded memory under a peer flood.
            q.pop_front();
        }
        q.push_back(env.body); }
}
fn push_frame_bytes(line: &mut Vec<u8>, bytes: &[u8]) -> std::io::Result<bool> {
    if let Some(end) = bytes.iter().position(|byte| *byte == b'\\n') {
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
    use super::{push_frame_bytes, MAX_FRAME};

    #[test]
    fn accepts_frame_at_limit_with_delimiter() {
        let mut frame = Vec::new();
        assert!(!push_frame_bytes(&mut frame, &vec![b'x'; MAX_FRAME]).unwrap());
        assert!(push_frame_bytes(&mut frame, b"\\n").unwrap());
        assert_eq!(frame.len(), MAX_FRAME + 1);
        assert_eq!(frame.last(), Some(&b'\\n'));
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
        let error = push_frame_bytes(&mut frame, b"x\\n").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(frame.len(), MAX_FRAME);
    }
}
