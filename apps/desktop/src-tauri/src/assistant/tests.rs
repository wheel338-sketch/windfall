//! Written, not executed. HTTP fixtures bind only 127.0.0.1; never external services.
use super::connection::{Connection, endpoint};
use super::provider::Provider;
use super::*;
use std::io::{BufRead, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};
use windfall_ipc::AssistantConnectionUpdate;
use windfall_project::{Project, proposal::ProposalStamp};

const KEY: &str = "unit-test-private-key";
const REPLY: &str = r#"{"answer":"Proposed tempo","label":"Tempo","explanation":"Review tempo","commands":[{"type":"updateSettings","patch":{"tempoBpm":127}}]}"#;
fn context(project: &Project) -> AssistantContext<'_> {
    AssistantContext {
        project,
        stamp: ProposalStamp {
            generation: 4,
            revision: 0,
        },
        help: "",
    }
}
pub(crate) fn fake(body: String, delay: Duration) -> (String, thread::JoinHandle<String>) {
    fake_status(body, delay, "200 OK")
}
fn fake_status(
    body: String,
    delay: Duration,
    status: &'static str,
) -> (String, thread::JoinHandle<String>) {
    let (url, worker, _) = fake_response(body, delay, status, None);
    (url, worker)
}
fn fake_response(
    body: String,
    delay: Duration,
    status: &'static str,
    location: Option<String>,
) -> (
    String,
    thread::JoinHandle<String>,
    std::sync::mpsc::Receiver<()>,
) {
    let (notice, accepted) = std::sync::mpsc::channel();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut socket = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(2))
                }
                Err(e) => panic!("fake accept: {e}"),
            }
        };
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let request = read_request(&mut socket);
        let _ = notice.send(());
        thread::sleep(delay);
        let location = location
            .map(|url| format!("Location: {url}\r\n"))
            .unwrap_or_default();
        let _ = write!(
            socket,
            "HTTP/1.1 {status}\r\n{location}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        request
    });
    (url, worker, accepted)
}
fn read_request(socket: &mut TcpStream) -> String {
    let mut reader = std::io::BufReader::new(socket);
    let mut headers = String::new();
    let mut length = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(headers.len() < 16_384);
        if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse::<usize>().unwrap();
        }
        headers.push_str(&line);
        if line == "\r\n" {
            break;
        }
    }
    assert!(length <= 65_536);
    let mut body = vec![0; length];
    reader.read_exact(&mut body).unwrap();
    format!("{headers}{}", String::from_utf8(body).unwrap())
}
pub(crate) fn completion(content: &str) -> String {
    serde_json::json!({"choices":[{"message":{"content":content}}]}).to_string()
}
fn connection(base: String) -> Connection {
    Connection {
        base_url: base,
        model: "user-chosen-model".into(),
        key: KEY.into(),
        enabled: true,
    }
}

#[test]
fn fake_chat_completion_sends_prompt_schema_and_sensitive_header_and_returns_review_only() {
    let (url, server) = fake(completion(REPLY), Duration::ZERO);
    let project = Project::new("Local fixture");
    let before = project.clone();
    let service = Service::default();
    let request = service.begin();
    let config = connection(url);
    let reply = Provider {
        config: &config,
        timeout: Duration::from_secs(2),
        service: &service,
        request,
    }
    .respond("set tempo 127", context(&project))
    .unwrap();
    assert_eq!(reply.proposal.unwrap().stamp.generation, 4);
    assert_eq!(project, before);
    let sent = server.join().unwrap();
    assert!(sent.starts_with("POST /v1/chat/completions "));
    assert!(
        sent.to_ascii_lowercase()
            .contains(&format!("authorization: bearer {KEY}"))
    );
    let body = sent.split_once("\r\n\r\n").unwrap().1;
    assert!(body.contains("commandSchema"));
    assert!(body.contains("set tempo 127"));
    assert!(!body.contains(KEY));
    assert!(!body.contains("Local fixture"));
}

#[test]
fn fake_endpoint_malformed_unsupported_sensitive_and_oversized_replies_are_redacted_errors() {
    let service = Service::default();
    let project = Project::new("Bad replies");
    for body in [
        "{".into(),
        completion("{}"),
        completion(&REPLY.replace("updateSettings", "batch")),
        completion(&REPLY.replace("Proposed tempo", KEY)),
        "x".repeat(600_001),
    ] {
        let (url, server) = fake(body, Duration::ZERO);
        let config = connection(url);
        let request = service.begin();
        let error = Provider {
            config: &config,
            timeout: Duration::from_secs(2),
            service: &service,
            request,
        }
        .respond("tempo", context(&project))
        .unwrap_err();
        assert!(!error.contains(KEY));
        assert!(!error.contains("http://"));
        server.join().unwrap();
    }
}

#[test]
fn fake_endpoint_timeout_and_cancel_are_bounded_and_no_key_never_connects() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut config = connection(format!("http://{}/v1", listener.local_addr().unwrap()));
    config.key.clear();
    let service = Service::default();
    let project = Project::new("Timeout");
    let request = service.begin();
    assert!(
        Provider {
            config: &config,
            timeout: Duration::from_millis(30),
            service: &service,
            request
        }
        .respond("tempo", context(&project))
        .unwrap_err()
        .contains("OFF")
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    let (url, server) = fake(completion(REPLY), Duration::from_millis(250));
    let config = connection(url);
    let began = Instant::now();
    assert!(
        Provider {
            config: &config,
            timeout: Duration::from_millis(30),
            service: &service,
            request
        }
        .respond("tempo", context(&project))
        .unwrap_err()
        .contains("timed out")
    );
    assert!(began.elapsed() < Duration::from_secs(1));
    server.join().unwrap();
    let (url, server, accepted) = fake_response(
        completion(REPLY),
        Duration::from_millis(250),
        "200 OK",
        None,
    );
    let config = connection(url);
    let request = service.begin();
    thread::scope(|scope| {
        let service_ref = &service;
        scope.spawn(move || {
            accepted.recv_timeout(Duration::from_secs(2)).unwrap();
            service_ref.cancel(request);
        });
        assert!(
            Provider {
                config: &config,
                timeout: Duration::from_secs(2),
                service: &service,
                request
            }
            .respond("tempo", context(&project))
            .unwrap_err()
            .contains("cancelled")
        );
    });
    server.join().unwrap();
}

#[test]
fn connection_is_off_without_key_and_atomic_private_file_roundtrips_without_exposing_it() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("assistant-connection.json");
    let service = Service::default();
    assert_eq!(
        service.status(&path).unwrap(),
        windfall_ipc::AssistantConnection::default()
    );
    let update = |key| AssistantConnectionUpdate {
        base_url: "https://example.invalid/v1".into(),
        model: "chosen".into(),
        enabled: true,
        api_key: key,
    };
    assert!(!service.configure(&path, update(None)).unwrap().enabled);
    let status = service.configure(&path, update(Some(KEY.into()))).unwrap();
    assert!(status.enabled && status.has_key);
    assert!(!serde_json::to_string(&status).unwrap().contains(KEY));
    assert!(fs_key(&path));
    assert!(service.configure(&path, update(None)).unwrap().has_key);
    assert!(
        !service
            .configure(&path, update(Some(String::new())))
            .unwrap()
            .enabled
    );
    assert!(!service.status(&path).unwrap().has_key);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    for url in [
        "http://example.invalid/v1",
        "https://key@example.invalid/v1",
        "https://example.invalid/v1?key=secret",
        "file:///tmp/server",
    ] {
        assert!(endpoint(url).is_err());
    }
}
fn fs_key(path: &std::path::Path) -> bool {
    std::fs::read_to_string(path).unwrap().contains(KEY)
}

#[test]
fn http_refusal_and_redirect_never_expose_response_body_or_follow_another_endpoint() {
    let service = Service::default();
    let project = Project::new("Refusal");
    for status in ["401 Unauthorized", "302 Found"] {
        let trap = TcpListener::bind("127.0.0.1:0").unwrap();
        trap.set_nonblocking(true).unwrap();
        let (url, server, _) = fake_response(
            format!("sensitive failure: {KEY}"),
            Duration::ZERO,
            status,
            Some(format!("http://{}/trap", trap.local_addr().unwrap())),
        );
        let config = connection(url);
        let error = Provider {
            config: &config,
            timeout: Duration::from_secs(2),
            service: &service,
            request: service.begin(),
        }
        .respond("help", context(&project))
        .unwrap_err();
        assert!(error.contains("refused"));
        assert!(!error.contains(KEY));
        assert_eq!(
            trap.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        server.join().unwrap();
    }
}
