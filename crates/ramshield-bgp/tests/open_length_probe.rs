use ramshield_bgp::session::BgpSession;
use std::time::Duration;
use tokio::io::AsyncReadExt;

#[tokio::test]
async fn open_header_length_matches_bytes() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let mut sess = BgpSession::new(65001, 65002, addr.ip(), addr.port());
    sess.connect().await.unwrap();
    sess.send_open(4, 90).await.unwrap();
    let (mut sock, _) = listener.accept().await.unwrap();
    let mut buf = [0u8; 64];
    let n = tokio::time::timeout(Duration::from_secs(1), sock.read(&mut buf))
        .await
        .unwrap()
        .unwrap();
    let len = u16::from_be_bytes([buf[16], buf[17]]) as usize;
    assert_eq!(len, n, "OPEN length field must equal bytes on wire");
}
