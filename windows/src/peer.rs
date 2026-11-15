//! Optional opt-in local peer protocol; no internet speed-test service is used.
use crate::{
    error::{AppError, Result},
    heatmap::Cancellation,
    model::unix_ms,
};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    net::{IpAddr, SocketAddr, TcpListener, TcpStream},
    time::{Duration, Instant},
};
const MAX_BYTES: u32 = 8 * 1024 * 1024;
const MAGIC: &[u8; 8] = b"WIFIMAP1";
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeResult {
    pub peer: String,
    pub collected_unix_ms: u64,
    pub median_rtt_ms: f64,
    pub payload_mbps: f64,
    pub received_bytes: u32,
    pub elapsed_ms: u64,
}
impl ProbeResult {
    pub fn valid(&self) -> bool {
        self.peer.len() <= 128
            && self.collected_unix_ms > 0
            && self.median_rtt_ms.is_finite()
            && self.median_rtt_ms >= 0.0
            && self.payload_mbps.is_finite()
            && self.payload_mbps >= 0.0
            && self.received_bytes <= MAX_BYTES
            && self.elapsed_ms <= 60_000
    }
}
fn err(e: impl ToString) -> AppError {
    AppError::detail("peer_error", e)
}
pub fn local_address(text: &str) -> Result<SocketAddr> {
    let address: SocketAddr = text.parse().map_err(|_| AppError::new("peer_address"))?;
    let local = match address.ip() {
        IpAddr::V4(v) => v.is_loopback() || v.is_private() || v.is_link_local(),
        IpAddr::V6(v) => v.is_loopback() || v.is_unique_local() || v.is_unicast_link_local(),
    };
    if !local || address.port() == 0 {
        return Err(AppError::new("peer_address"));
    }
    Ok(address)
}
pub fn parse_token(text: &str) -> Result<[u8; 16]> {
    if text.len() != 32 || !text.is_ascii() {
        return Err(AppError::new("peer_token"));
    }
    let mut result = [0; 16];
    for (i, b) in result.iter_mut().enumerate() {
        *b = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16)
            .map_err(|_| AppError::new("peer_token"))?;
    }
    Ok(result)
}
fn configure(stream: &TcpStream) -> Result<()> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(err)?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(err)?;
    stream.set_nodelay(true).map_err(err)
}
fn check(cancel: &Cancellation, start: Instant) -> Result<()> {
    if cancel.is_cancelled() {
        return Err(AppError::new("cancelled"));
    }
    if start.elapsed() > Duration::from_secs(20) {
        return Err(AppError::new("peer_timeout"));
    }
    Ok(())
}
pub fn probe(address: SocketAddr, token: [u8; 16], cancel: &Cancellation) -> Result<ProbeResult> {
    local_address(&address.to_string())?;
    let start = Instant::now();
    check(cancel, start)?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).map_err(err)?;
    configure(&stream)?;
    probe_io(&mut stream, &address.to_string(), token, cancel, start)
}
fn probe_io(
    stream: &mut (impl Read + Write),
    peer: &str,
    token: [u8; 16],
    cancel: &Cancellation,
    start: Instant,
) -> Result<ProbeResult> {
    stream.write_all(MAGIC).map_err(err)?;
    stream.write_all(&token).map_err(err)?;
    let mut ack = [0; 1];
    stream.read_exact(&mut ack).map_err(err)?;
    if ack != [1] {
        return Err(AppError::new("peer_token"));
    }
    let mut times = Vec::new();
    for i in 0..5u8 {
        check(cancel, start)?;
        let now = Instant::now();
        stream.write_all(&[b'E', i]).map_err(err)?;
        stream.read_exact(&mut ack).map_err(err)?;
        if ack != [i] {
            return Err(AppError::new("peer_protocol"));
        }
        times.push(now.elapsed().as_secs_f64() * 1000.0);
    }
    check(cancel, start)?;
    stream.write_all(b"D").map_err(err)?;
    stream.write_all(&MAX_BYTES.to_be_bytes()).map_err(err)?;
    let transfer_start = Instant::now();
    let mut received = 0;
    let mut buffer = [0; 64 * 1024];
    while received < MAX_BYTES {
        check(cancel, start)?;
        let cap = buffer.len().min((MAX_BYTES - received) as usize);
        let n = stream.read(&mut buffer[..cap]).map_err(err)?;
        if n == 0 {
            return Err(AppError::new("peer_incomplete"));
        }
        received += n as u32;
    }
    times.sort_by(f64::total_cmp);
    Ok(ProbeResult {
        peer: peer.into(),
        collected_unix_ms: unix_ms(),
        median_rtt_ms: times[2],
        payload_mbps: received as f64 * 8.0
            / transfer_start.elapsed().as_secs_f64().max(0.000_001)
            / 1_000_000.0,
        received_bytes: received,
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}
pub fn serve_connection(mut stream: TcpStream, token: [u8; 16]) -> Result<()> {
    configure(&stream)?;
    serve_io(&mut stream, token)
}
fn serve_io(stream: &mut (impl Read + Write), token: [u8; 16]) -> Result<()> {
    let start = Instant::now();
    let cancel = Cancellation::default();
    let mut header = [0; 24];
    stream.read_exact(&mut header).map_err(err)?;
    if &header[..8] != MAGIC
        || header[8..]
            .iter()
            .zip(token)
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            != 0
    {
        let _ = stream.write_all(&[0]);
        return Err(AppError::new("peer_token"));
    }
    stream.write_all(&[1]).map_err(err)?;
    for _ in 0..6 {
        check(&cancel, start)?;
        let mut command = [0; 1];
        stream.read_exact(&mut command).map_err(err)?;
        match command[0] {
            b'E' => {
                stream.read_exact(&mut command).map_err(err)?;
                stream.write_all(&command).map_err(err)?;
            }
            b'D' => {
                let mut count = [0; 4];
                stream.read_exact(&mut count).map_err(err)?;
                let mut count = u32::from_be_bytes(count);
                if count == 0 || count > MAX_BYTES {
                    return Err(AppError::new("peer_protocol"));
                }
                let data = [0x5a; 64 * 1024];
                while count > 0 {
                    check(&cancel, start)?;
                    let n = data.len().min(count as usize);
                    stream.write_all(&data[..n]).map_err(err)?;
                    count -= n as u32;
                }
                return Ok(());
            }
            _ => return Err(AppError::new("peer_protocol")),
        }
    }
    Err(AppError::new("peer_protocol"))
}
/// Explicit CLI helper, one client, bounded 10-minute accept window; no install,
/// firewall rule, service or elevation. A remote peer needs its own allowed port.
pub fn serve_once(address: SocketAddr, token: [u8; 16]) -> Result<()> {
    local_address(&address.to_string())?;
    let listener = TcpListener::bind(address).map_err(err)?;
    listener.set_nonblocking(true).map_err(err)?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(600) {
        match listener.accept() {
            Ok((stream, remote)) => {
                local_address(&remote.to_string())?;
                return serve_connection(stream, token);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(e) => return Err(err(e)),
        }
    }
    Err(AppError::new("peer_timeout"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg_attr(
        not(feature = "network-tests"),
        ignore = "requires loopback socket permission (WSAEACCES 10013 in the code-only environment)"
    )]
    fn loopback_peer_measures_payload_and_rtt() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let token = [3; 16];
        let worker = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            serve_connection(stream, token).unwrap();
        });
        let result = probe(address, token, &Cancellation::default()).unwrap();
        worker.join().unwrap();
        assert_eq!(result.received_bytes, MAX_BYTES);
        assert!(result.valid());
        assert!(result.payload_mbps > 0.0);
    }
    #[test]
    fn rejects_public_targets_bad_tokens_and_cancelled_work() {
        assert!(local_address("8.8.8.8:80").is_err());
        assert!(parse_token("short").is_err());
        let c = Cancellation::default();
        c.cancel();
        assert_eq!(
            probe("127.0.0.1:1234".parse().unwrap(), [0; 16], &c)
                .unwrap_err()
                .key,
            "cancelled"
        );
    }
    #[test]
    #[cfg_attr(
        not(feature = "network-tests"),
        ignore = "requires loopback socket permission (WSAEACCES 10013 in the code-only environment)"
    )]
    fn peer_authentication_failure_is_explicit() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            assert_eq!(
                serve_connection(stream, [1; 16]).unwrap_err().key,
                "peer_token"
            );
        });
        assert_eq!(
            probe(address, [2; 16], &Cancellation::default())
                .unwrap_err()
                .key,
            "peer_token"
        );
        worker.join().unwrap();
    }

    struct MemoryStream {
        tx: std::sync::mpsc::SyncSender<Vec<u8>>,
        rx: std::sync::mpsc::Receiver<Vec<u8>>,
        buffer: std::collections::VecDeque<u8>,
    }
    impl Read for MemoryStream {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if out.is_empty() {
                return Ok(0);
            }
            if self.buffer.is_empty() {
                match self.rx.recv_timeout(Duration::from_secs(2)) {
                    Ok(bytes) => self.buffer.extend(bytes),
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(0),
                    Err(_) => return Err(std::io::ErrorKind::TimedOut.into()),
                }
            }
            let n = out.len().min(self.buffer.len());
            for b in &mut out[..n] {
                *b = self.buffer.pop_front().unwrap();
            }
            Ok(n)
        }
    }
    impl Write for MemoryStream {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.tx
                .send(bytes.to_vec())
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    fn duplex() -> (MemoryStream, MemoryStream) {
        let (a, ar) = std::sync::mpsc::sync_channel(2);
        let (b, br) = std::sync::mpsc::sync_channel(2);
        (
            MemoryStream {
                tx: a,
                rx: br,
                buffer: Default::default(),
            },
            MemoryStream {
                tx: b,
                rx: ar,
                buffer: Default::default(),
            },
        )
    }
    #[test]
    fn memory_peer_runs_the_same_bounded_protocol() {
        let (mut client, mut server) = duplex();
        let worker = std::thread::spawn(move || serve_io(&mut server, [7; 16]));
        let result = probe_io(
            &mut client,
            "127.0.0.1:46820",
            [7; 16],
            &Cancellation::default(),
            Instant::now(),
        )
        .unwrap();
        assert_eq!(result.received_bytes, MAX_BYTES);
        assert!(result.valid());
        worker.join().unwrap().unwrap();
    }
    #[test]
    fn memory_peer_rejects_wrong_authentication() {
        let (mut client, mut server) = duplex();
        let worker = std::thread::spawn(move || serve_io(&mut server, [7; 16]));
        assert_eq!(
            probe_io(
                &mut client,
                "127.0.0.1:46820",
                [8; 16],
                &Cancellation::default(),
                Instant::now()
            )
            .unwrap_err()
            .key,
            "peer_token"
        );
        assert_eq!(worker.join().unwrap().unwrap_err().key, "peer_token");
    }
}
