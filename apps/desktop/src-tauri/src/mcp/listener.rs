//! The loopback listener. Like the phone remote's, it accepts on a
//! nonblocking socket polled from one thread, serves each connection on a
//! bounded worker, and stops when its session is gone or it is dropped.
//! Unlike the phone remote it binds `127.0.0.1` only, never a LAN address.

use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use windfall_mcp::Host;
use windfall_mcp::http::{self, Endpoint};

/// Connections served at once; one more is told to try again.
const WORKERS: usize = 8;
/// Connections accepted per second at most.
const PER_SECOND: u32 = 50;

pub(crate) struct Listener {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    pub port: u16,
}

impl Drop for Listener {
    /// Waits for the accept loop to close its socket, so the port can be
    /// bound again at once. The loop polls every 20 ms. When the session
    /// ends on the loop's own thread there is nothing to wait for.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take()
            && thread.thread().id() != std::thread::current().id()
        {
            let _ = thread.join();
        }
    }
}

/// Binds `127.0.0.1:port` and serves until the listener is dropped or
/// `alive` says the session has gone.
pub(crate) fn start(
    server: Arc<windfall_mcp::Server>,
    host: Arc<dyn Host>,
    alive: Box<dyn Fn() -> bool + Send>,
    port: u16,
    token: String,
) -> Result<Listener, String> {
    let socket = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
        .map_err(|_| "Windfall could not listen on that port. Another program may be using it; choose another port.")?;
    socket
        .set_nonblocking(true)
        .map_err(|_| "Windfall could not listen on that port. Another program may be using it; choose another port.")?;
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let token: Arc<str> = Arc::from(token);
    let active = Arc::new(AtomicUsize::new(0));
    let thread = std::thread::Builder::new()
        .name("windfall-mcp".into())
        .spawn(move || {
            let mut window = Instant::now();
            let mut accepted = 0u32;
            while !stopped.load(Ordering::Acquire) && alive() {
                match socket.accept() {
                    Ok((stream, peer)) => {
                        // Accepted sockets inherit nonblocking mode on Windows.
                        if !peer.ip().is_loopback() || stream.set_nonblocking(false).is_err() {
                            continue;
                        }
                        if window.elapsed() >= Duration::from_secs(1) {
                            window = Instant::now();
                            accepted = 0;
                        }
                        accepted += 1;
                        if accepted > PER_SECOND {
                            refuse(stream, 429, "Too many requests; slow down");
                            continue;
                        }
                        if active.load(Ordering::Acquire) >= WORKERS {
                            refuse(
                                stream,
                                503,
                                "Windfall is busy with other requests; try again",
                            );
                            continue;
                        }
                        active.fetch_add(1, Ordering::AcqRel);
                        let (server, host, token, active) =
                            (server.clone(), host.clone(), token.clone(), active.clone());
                        let worker_active = active.clone();
                        let spawned = std::thread::Builder::new()
                            .name("windfall-mcp-client".into())
                            .spawn(move || {
                                serve(stream, &server, &*host, port, &token);
                                worker_active.fetch_sub(1, Ordering::AcqRel);
                            });
                        if spawned.is_err() {
                            active.fetch_sub(1, Ordering::AcqRel);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(100)),
                }
            }
        })
        .map_err(|_| "Windfall could not start the MCP server.")?;
    Ok(Listener {
        stop,
        thread: Some(thread),
        port,
    })
}

fn refuse(mut stream: TcpStream, status: u16, message: &str) {
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    let _ = http::write_response(&mut stream, &http::refused(status, message));
    let _ = stream.shutdown(Shutdown::Both);
}

fn serve(
    mut stream: TcpStream,
    server: &windfall_mcp::Server,
    host: &dyn Host,
    port: u16,
    token: &str,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let response = match http::read_request(&mut stream, Instant::now() + http::READ_TIME) {
        Ok(request) => http::respond(server, host, &Endpoint { port, token }, &request),
        Err(refusal) => http::unreadable(refusal),
    };
    let _ = http::write_response(&mut stream, &response);
    let _ = stream.shutdown(Shutdown::Write);
}
