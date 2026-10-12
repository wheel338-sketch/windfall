//! Explicitly enabled local-network remote. Weak workers never keep the app
//! alive; revocation closes the listener, sessions and held notes.
use super::{Session, WeakSession};
use crate::{remote_protocol as wire, sync::lock};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};
use windfall_ipc::RemoteSettingsState;
use windfall_project::{ChannelId, Command, TrackId};

pub(super) struct Server {
    stop: Arc<AtomicBool>,
    address: SocketAddr,
    code: String,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
struct Client {
    sequence: u64,
    window: Instant,
    count: u32,
    expires: Instant,
}
struct Authority {
    code: String,
    expires: Instant,
    paired: bool,
    attempts: u32,
    clients: HashMap<String, Client>,
    window: Instant,
    connections: u32,
}
impl Authority {
    fn new() -> Result<Self, String> {
        let bytes = random()?;
        let number = u32::from_le_bytes(bytes[..4].try_into().expect("four bytes")) % 1_000_000;
        Ok(Self {
            code: format!("{number:06}"),
            expires: Instant::now() + Duration::from_secs(300),
            paired: false,
            attempts: 0,
            clients: HashMap::new(),
            window: Instant::now(),
            connections: 0,
        })
    }
    fn connection(&mut self) -> bool {
        if self.window.elapsed() >= Duration::from_secs(1) {
            self.window = Instant::now();
            self.connections = 0;
        }
        self.connections = self.connections.saturating_add(1);
        self.connections <= 20
    }
    fn pair(&mut self, code: &str) -> Result<String, String> {
        self.attempts = self.attempts.saturating_add(1);
        if self.attempts > 10
            || self.paired
            || Instant::now() >= self.expires
            || !constant_eq(code.as_bytes(), self.code.as_bytes())
        {
            return Err("Pairing refused. Restart the remote for a fresh code.".into());
        }
        let token = hex(&random()?);
        self.clients.insert(
            token.clone(),
            Client {
                sequence: 0,
                window: Instant::now(),
                count: 0,
                expires: Instant::now() + Duration::from_secs(8 * 3600),
            },
        );
        self.paired = true;
        Ok(token)
    }
    fn admit(&mut self, token: &str, sequence: u64) -> Result<(), String> {
        let now = Instant::now();
        let client = self
            .clients
            .get_mut(token)
            .ok_or("Remote authentication failed.")?;
        if now >= client.expires {
            return Err("Remote session expired.".into());
        }
        if sequence <= client.sequence {
            return Err("Remote replay refused.".into());
        }
        if now.duration_since(client.window) >= Duration::from_secs(1) {
            client.window = now;
            client.count = 0;
        }
        client.count += 1;
        if client.count > 40 {
            return Err("Remote rate limit exceeded.".into());
        }
        client.sequence = sequence;
        Ok(())
    }
}
fn random() -> Result<[u8; 32], String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |d, (a, b)| d | (a ^ b)) == 0
}

#[derive(Debug, Deserialize)]
#[serde(tag = "name", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum Action {
    Feed {},
    Play {},
    Stop {},
    Fader {
        track: TrackId,
        value: f32,
    },
    NoteOn {
        channel: ChannelId,
        key: u8,
        velocity: f32,
    },
    NoteOff {
        channel: ChannelId,
        key: u8,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    token: String,
    sequence: u64,
    generation: u64,
    action: Action,
}
type Notes = HashMap<(u64, ChannelId, u8), Instant>;

impl Session {
    pub fn phone_remote_state(&self) -> RemoteSettingsState {
        let server = lock(&self.inner.phone_remote);
        match server.as_ref() {
            Some(s) => RemoteSettingsState {
                enabled: true,
                address: Some(format!("http://{}", s.address)),
                pairing_code: Some(s.code.clone()),
            },
            None => RemoteSettingsState {
                enabled: false,
                address: None,
                pairing_code: None,
            },
        }
    }
    pub fn phone_remote_set(&self, address: Option<&str>) -> Result<RemoteSettingsState, String> {
        let mut current = lock(&self.inner.phone_remote);
        if address.is_none() {
            current.take();
            drop(current);
            self.unsubscribe_realtime("phone-remote");
            return Ok(self.phone_remote_state());
        }
        if current.is_some() {
            return Err("Disable the remote before changing its bind address.".into());
        }
        let address: SocketAddr = address.expect("some").parse().map_err(|_| {
            "Enter a local IPv4 address and port, for example 192.168.1.4:8787.".to_string()
        })?;
        match address.ip() {
            IpAddr::V4(ip) if ip.is_private() || ip.is_loopback() || ip.is_link_local() => {}
            _ => return Err(
                "Remote binding requires a specific private, link-local or loopback IPv4 address."
                    .into(),
            ),
        }
        let listener = TcpListener::bind(address).map_err(|e| e.to_string())?;
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        let address = listener.local_addr().map_err(|e| e.to_string())?;
        let authority = Authority::new()?;
        let code = authority.code.clone();
        let authority = Arc::new(Mutex::new(authority));
        let stop = Arc::new(AtomicBool::new(false));
        let feed = Arc::new(Mutex::new(json!({"playing":false,"tick":0,"meters":[]})));
        let weak_feed = Arc::downgrade(&feed);
        self.subscribe_realtime(
            "phone-remote",
            Box::new(move |frame| {
                let Some(feed) = weak_feed.upgrade() else {
                    return false;
                };
                if let Ok(mut latest) = feed.try_lock() {
                    *latest =
                        json!({"playing":frame.playing,"tick":frame.tick,"meters":frame.meters});
                }
                true
            }),
        );
        let weak = self.downgrade();
        let stopped = stop.clone();
        let active = Arc::new(AtomicUsize::new(0));
        let launched = std::thread::Builder::new()
            .name("windfall-phone-remote".into())
            .spawn(move || {
                while !stopped.load(Ordering::Acquire) && weak.upgrade().is_some() {
                    match listener.accept() {
                        Ok((stream, peer)) => {
                            // Accepted sockets inherit the listener's nonblocking
                            // mode on Windows. Each client has a bounded worker.
                            if stream.set_nonblocking(false).is_err() {
                                continue;
                            }
                            if !local_peer(peer.ip())
                                || active.load(Ordering::Acquire) >= 4
                                || !lock(&authority).connection()
                            {
                                drop(stream);
                                continue;
                            }
                            active.fetch_add(1, Ordering::AcqRel);
                            let (weak, authority, feed, stopped, active) = (
                                WeakSession {
                                    inner: weak.inner.clone(),
                                },
                                authority.clone(),
                                feed.clone(),
                                stopped.clone(),
                                active.clone(),
                            );
                            let worker_active = active.clone();
                            if std::thread::Builder::new()
                                .name("windfall-remote-client".into())
                                .spawn(move || {
                                    let _ = serve(stream, address, weak, authority, feed, stopped);
                                    worker_active.fetch_sub(1, Ordering::AcqRel);
                                })
                                .is_err()
                            {
                                active.fetch_sub(1, Ordering::AcqRel);
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(25))
                        }
                        Err(_) => break,
                    }
                }
            });
        if let Err(e) = launched {
            self.unsubscribe_realtime("phone-remote");
            return Err(e.to_string());
        }
        *current = Some(Server {
            stop,
            address,
            code,
        });
        drop(current);
        Ok(self.phone_remote_state())
    }
    pub(super) fn remote_action(&self, generation: u64, action: &Action) -> Result<(), String> {
        match action {
            Action::Feed {} => Ok(()),
            Action::Play {} => self.transport_play_checked(Some(generation)).map(|_| ()),
            Action::Stop {} => self.transport_stop_checked(Some(generation)).map(|_| ()),
            Action::Fader { track, value } => {
                if !value.is_finite() || !(0.0..=2.0).contains(value) {
                    return Err("Remote fader is outside 0..2.".into());
                }
                let ticket = {
                    let _recording = self.recording_idle()?;
                    let state = self.state();
                    if state.generation != generation {
                        return Err("Remote project generation is stale.".into());
                    }
                    // The identical checked Command used by mixer UI faders.
                    self.sample_edit_ticket(
                        &state,
                        Command::UpdateMixerTrack {
                            id: *track,
                            patch: windfall_project::MixerTrackPatch {
                                volume: Some(*value),
                                ..Default::default()
                            },
                        },
                        None,
                        Vec::new(),
                    )?
                };
                self.finish_sample_edit(ticket).map(|_| ())
            }
            Action::NoteOn {
                channel,
                key,
                velocity,
            } => self.remote_note(generation, *channel, *key, Some(*velocity)),
            Action::NoteOff { channel, key } => self.remote_note(generation, *channel, *key, None),
        }
    }
    fn remote_note(
        &self,
        generation: u64,
        channel: ChannelId,
        key: u8,
        velocity: Option<f32>,
    ) -> Result<(), String> {
        if key > 127 || velocity.is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v)) {
            return Err("Invalid remote note.".into());
        }
        let _recording = velocity.map(|_| self.recording_idle()).transpose()?;
        let state = self.state();
        if state.generation != generation || state.document.project().channel(channel).is_none() {
            return Err("Remote note target is stale.".into());
        }
        match velocity {
            Some(v) => self.audition_note_on(channel, key, v),
            None => self.audition_note_off(channel, key),
        }
        Ok(())
    }
}
fn local_peer(ip: IpAddr) -> bool {
    matches!(ip, IpAddr::V4(ip) if ip.is_private() || ip.is_loopback() || ip.is_link_local())
}
fn serve(
    mut stream: TcpStream,
    address: SocketAddr,
    weak: WeakSession,
    authority: Arc<Mutex<Authority>>,
    feed: Arc<Mutex<Value>>,
    stop: Arc<AtomicBool>,
) -> Result<(), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    let request = wire::request(&mut stream)?;
    let host = address.to_string();
    if request.headers.get("host") != Some(&host)
        || request
            .headers
            .get("origin")
            .is_some_and(|o| o != &format!("http://{host}"))
    {
        return Err("Remote origin/host refused".into());
    }
    if request.method == "GET" && request.path == "/" {
        return wire::response(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            include_bytes!("phone_remote.html"),
        );
    }
    if request.method == "POST" && request.path == "/pair" {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Pair {
            code: String,
        }
        let result = serde_json::from_slice::<Pair>(&request.body)
            .map_err(|e| e.to_string())
            .and_then(|pair| lock(&authority).pair(&pair.code));
        let (status, body) = match result {
            Ok(token) => ("200 OK", json!({"token":token})),
            Err(e) => ("403 Forbidden", json!({"error":e})),
        };
        return wire::response(
            &mut stream,
            status,
            "application/json",
            body.to_string().as_bytes(),
        );
    }
    if request.method != "GET" || request.path != "/ws" {
        return wire::response(&mut stream, "404 Not Found", "text/plain", b"Not found");
    }
    wire::upgrade(&mut stream, &request)?;
    let mut notes = Notes::new();
    let result = (|| {
        while !stop.load(Ordering::Acquire) {
            let Some(text) = wire::read_frame(&mut wire::Deadline::new(&mut stream))? else {
                break;
            };
            if stop.load(Ordering::Acquire) {
                break;
            }
            let envelope: Envelope = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            lock(&authority).admit(&envelope.token, envelope.sequence)?;
            let Some(session) = weak.upgrade() else {
                break;
            };
            // Notes are leased for five seconds; feed requests also expire them.
            release(&session, &mut notes, false);
            let remote = lock(&session.inner.phone_remote);
            if !remote
                .as_ref()
                .is_some_and(|server| Arc::ptr_eq(&server.stop, &stop))
            {
                break;
            }
            let outcome = session.remote_action(envelope.generation, &envelope.action);
            drop(remote);
            if outcome.is_ok() {
                match envelope.action {
                    Action::NoteOn { channel, key, .. } => {
                        if notes.len() >= 32 {
                            let _ = session.remote_note(envelope.generation, channel, key, None);
                            return Err("Remote polyphony limit exceeded".into());
                        }
                        notes.insert((envelope.generation, channel, key), Instant::now());
                    }
                    Action::NoteOff { channel, key } => {
                        notes.remove(&(envelope.generation, channel, key));
                    }
                    _ => {}
                }
            }
            let state = session.state();
            let project = state.document.project();
            let body = json!({"sequence":envelope.sequence,"error":outcome.err(),"generation":state.generation,"transport":session.transport_state(),"feed":*lock(&feed),"tracks":project.mixer.tracks.iter().take(128).map(|t| json!({"id":t.id,"name":t.name,"gain":t.volume})).collect::<Vec<_>>(),"channels":project.channels.iter().take(128).map(|c| json!({"id":c.id,"name":c.name})).collect::<Vec<_>>()});
            drop(state);
            drop(session);
            wire::text_frame(&mut stream, &body.to_string())?;
        }
        Ok(())
    })();
    if let Some(session) = weak.upgrade() {
        release(&session, &mut notes, true);
    }
    result
}
fn release(session: &Session, notes: &mut Notes, all: bool) {
    notes.retain(|&(generation, channel, key), started| {
        if all || started.elapsed() >= Duration::from_secs(5) {
            let _ = session.remote_note(generation, channel, key, None);
            false
        } else {
            true
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pairing_attempt_expiry_connection_and_action_shape_limits() {
        let mut auth = Authority::new().unwrap();
        let code = auth.code.clone();
        for _ in 0..10 {
            assert!(auth.pair("wrong").is_err());
        }
        assert!(auth.pair(&code).is_err());
        let mut auth = Authority::new().unwrap();
        let code = auth.code.clone();
        auth.expires = Instant::now();
        assert!(auth.pair(&code).is_err());
        for _ in 0..20 {
            assert!(auth.connection());
        }
        assert!(!auth.connection());
        assert!(serde_json::from_str::<Action>(r#"{"name":"feed","track":0}"#).is_err());
        assert!(
            serde_json::from_str::<Envelope>(
                r#"{"token":"x","sequence":1,"generation":0,"action":{"name":"feed"},"extra":true}"#
            )
            .is_err()
        );
        assert!(!local_peer("203.0.113.1".parse().unwrap()));
        assert!(!local_peer("::1".parse().unwrap()));
    }
    #[test]
    fn authentication_replay_expiry_and_rate_limits() {
        let mut auth = Authority::new().unwrap();
        let code = auth.code.clone();
        assert!(auth.pair("wrong").is_err());
        let token = auth.pair(&code).unwrap();
        assert_eq!(token.len(), 64);
        assert!(auth.pair(&code).is_err());
        assert!(auth.admit("wrong", 1).is_err());
        auth.admit(&token, 1).unwrap();
        assert!(auth.admit(&token, 1).is_err());
        for sequence in 2..=40 {
            auth.admit(&token, sequence).unwrap();
        }
        assert!(auth.admit(&token, 41).is_err());
        auth.clients.get_mut(&token).unwrap().expires = Instant::now();
        assert!(auth.admit(&token, 42).is_err());
    }
}
