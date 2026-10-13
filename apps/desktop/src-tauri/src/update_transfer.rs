//! HTTPS transfer is bounded before any response bytes enter an accumulating
//! buffer. The updater only sees one-shot, already bounded loopback replies,
//! retaining its production Minisign verifier without a second crypto stack.
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use url::Url;

pub(crate) const PACKAGE_CAP: usize = 512 * 1024 * 1024;
pub(crate) const MANIFEST_CAP: usize = 1024 * 1024;

pub(crate) async fn fetch(
    url: Url,
    cap: usize,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<Option<Vec<u8>>, String> {
    super::updates::validate_redirect(&url, 0).map_err(str::to_owned)?;
    fetch_client(
        url,
        cap,
        &reqwest::Client::builder()
            .redirect(super::updates::update_redirect_policy())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|error| error.to_string())?,
        &mut progress,
    )
    .await
}
async fn fetch_client(
    url: Url,
    cap: usize,
    client: &reqwest::Client,
    progress: &mut impl FnMut(u64, Option<u64>),
) -> Result<Option<Vec<u8>>, String> {
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(None);
    }
    response
        .error_for_status_ref()
        .map_err(|error| error.to_string())?;
    let total = response.content_length();
    if total.is_some_and(|total| total > cap as u64) {
        return Err("Update response exceeds its byte budget".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        append(&mut bytes, &chunk, cap)?;
        progress(bytes.len() as u64, total);
    }
    if total.is_some_and(|total| total != bytes.len() as u64) {
        return Err("Update response was truncated".into());
    }
    if bytes.is_empty() {
        return Err("Update response is empty".into());
    }
    Ok(Some(bytes))
}
fn append(bytes: &mut Vec<u8>, chunk: &[u8], cap: usize) -> Result<(), String> {
    if chunk.len() > cap.saturating_sub(bytes.len()) {
        return Err("Update response exceeds its byte budget".into());
    }
    bytes
        .try_reserve_exact(chunk.len())
        .map_err(|_| "Cannot reserve bounded update response".to_owned())?;
    bytes.extend_from_slice(chunk);
    Ok(())
}
// An idle read timeout alone is insufficient: a peer can continually send
// one byte just before it expires. Cancellation and the absolute replay
// deadline must bound every header read, including incomplete capability URLs.
fn read_request(
    stream: &mut std::net::TcpStream,
    stopped: &AtomicBool,
    deadline: Instant,
) -> std::io::Result<Vec<u8>> {
    let mut request = Vec::new();
    let mut byte = [0];
    while request.len() < 8192 && !request.ends_with(b"\r\n\r\n") {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if stopped.load(Ordering::Acquire) || remaining.is_zero() {
            return Err(std::io::Error::from(std::io::ErrorKind::TimedOut));
        }
        stream.set_read_timeout(Some(remaining.min(Duration::from_secs(1))))?;
        stream.read_exact(&mut byte)?;
        request.push(byte[0]);
    }
    if !request.ends_with(b"\r\n\r\n") {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
    }
    Ok(request)
}

/// The URL is an unguessable capability served only on 127.0.0.1. No redirects,
/// proxy, network trust exceptions or remote endpoints participate in replay.
pub(crate) struct Reply {
    pub url: Url,
    stopped: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Reply {
    pub fn new(bytes: Vec<u8>) -> Result<Self, String> {
        let listener =
            TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).map_err(|e| e.to_string())?;
        let path = secret
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>();
        let url = Url::parse(&format!(
            "http://127.0.0.1:{}/{path}",
            listener.local_addr().map_err(|e| e.to_string())?.port()
        ))
        .map_err(|e| e.to_string())?;
        let stopped = Arc::new(AtomicBool::new(false));
        let cancel = stopped.clone();
        let worker=std::thread::Builder::new().name("windfall-update-verification".into()).spawn(move||{
            let deadline=Instant::now()+Duration::from_secs(15);
            while !cancel.load(Ordering::Acquire) && Instant::now()<deadline {
                match listener.accept() {
                    Ok((mut stream,_))=>{
                        let _=stream.set_read_timeout(Some(Duration::from_secs(1)));
                        let _=stream.set_write_timeout(Some(Duration::from_secs(2)));
                        let Ok(request) = read_request(&mut stream, &cancel, deadline) else { continue; };
                        let expected=format!("GET /{path} HTTP/1.1\r\n");
                        if !request.starts_with(expected.as_bytes()){ let _=stream.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n"); continue; }
                        let header=format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",bytes.len());
                        if stream.write_all(header.as_bytes()).is_ok(){ let _=stream.write_all(&bytes); }
                        break;
                    }
                    Err(e) if e.kind()==std::io::ErrorKind::WouldBlock=>std::thread::sleep(Duration::from_millis(10)),
                    Err(_)=>break,
                }
            }
        }).map_err(|e|e.to_string())?;
        Ok(Self {
            url,
            stopped,
            worker: Some(worker),
        })
    }
}
impl Drop for Reply {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dripfed_replay_headers_cannot_extend_the_absolute_deadline() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let mut client = std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        client
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let (mut server, _) = listener.accept().unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let reader_stop = stopped.clone();
        let (send, receive) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let result = read_request(
                &mut server,
                &reader_stop,
                Instant::now() + Duration::from_millis(50),
            );
            send.send(result.map_err(|error| error.kind())).unwrap();
        });
        let writer_stop = stopped.clone();
        let writer = std::thread::spawn(move || {
            while !writer_stop.load(Ordering::Acquire) {
                if client.write_all(b"G").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        let result = receive.recv_timeout(Duration::from_secs(1));
        stopped.store(true, Ordering::Release);
        writer.join().unwrap();
        reader.join().unwrap();
        assert_eq!(result.unwrap().unwrap_err(), std::io::ErrorKind::TimedOut);
    }

    #[test]
    fn streamed_chunks_are_refused_before_buffer_growth_including_a_missing_length() {
        let mut bytes = vec![1; 3];
        let capacity = bytes.capacity();
        assert!(append(&mut bytes, &[2; 8], 10).is_err());
        assert_eq!(bytes, vec![1; 3]);
        assert_eq!(bytes.capacity(), capacity);
        append(&mut bytes, &[2; 7], 10).unwrap();
        assert_eq!(bytes.len(), 10);
        assert!(append(&mut bytes, &[3], 10).is_err());
        assert_eq!(bytes.len(), 10);
    }
    #[test]
    fn oversized_streamed_http_bodies_never_report_or_store_more_than_the_budget() {
        tauri::async_runtime::block_on(async {
            for chunked in [false, true] {
                let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
                let url = Url::parse(&format!(
                    "http://{}/oversize",
                    listener.local_addr().unwrap()
                ))
                .unwrap();
                let server = std::thread::spawn(move || {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut request = [0; 4096];
                    let _ = stream.read(&mut request);
                    if chunked {
                        let _=stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n8\r\n12345678\r\n8\r\n12345678\r\n0\r\n\r\n");
                    } else {
                        let _=stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 16\r\nConnection: close\r\n\r\n1234567812345678");
                    }
                });
                let client = reqwest::Client::builder()
                    .timeout(Duration::from_secs(2))
                    .build()
                    .unwrap();
                let mut seen = 0;
                let result = fetch_client(url, 12, &client, &mut |bytes, _| {
                    assert!(bytes <= 12);
                    seen = bytes;
                })
                .await;
                assert!(result.unwrap_err().contains("byte budget"));
                assert!(seen <= 12);
                if !chunked {
                    assert_eq!(seen, 0, "Content-Length is refused before reading the body");
                }
                server.join().unwrap();
            }
        });
    }
    #[test]
    fn bounded_http_transfer_rejects_a_real_redirect_loop_and_truncated_body() {
        tauri::async_runtime::block_on(async {
            let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
            let url =
                Url::parse(&format!("http://{}/loop", listener.local_addr().unwrap())).unwrap();
            let location = url.clone();
            let server = std::thread::spawn(move || {
                for _ in 0..4 {
                    let (mut stream, _) = listener.accept().unwrap();
                    let mut request = [0; 4096];
                    let _ = stream.read(&mut request);
                    write!(
                        stream,
                        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\n\r\n"
                    )
                    .unwrap();
                }
            });
            // Exercise the production credential/count validator with a local
            // HTTP transport. Only its scheme is normalized: no TLS fixture
            // or operator host participates in this deterministic loop.
            let redirects = reqwest::redirect::Policy::custom(|attempt| {
                let mut checked = attempt.url().clone();
                checked.set_scheme("https").unwrap();
                match super::super::updates::validate_redirect(&checked, attempt.previous().len()) {
                    Ok(()) => attempt.follow(),
                    Err(reason) => attempt.error(reason),
                }
            });
            let client = reqwest::Client::builder()
                .redirect(redirects)
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap();
            assert!(
                fetch_client(url, 64, &client, &mut |_, _| {})
                    .await
                    .is_err()
            );
            server.join().unwrap();
            let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
            let url = Url::parse(&format!(
                "http://{}/truncated",
                listener.local_addr().unwrap()
            ))
            .unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                let _ = stream.read(&mut request);
                stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\nshort",
                    )
                    .unwrap();
            });
            assert!(
                fetch_client(url, 64, &client, &mut |_, _| {})
                    .await
                    .is_err()
            );
            server.join().unwrap();
        });
    }
}
