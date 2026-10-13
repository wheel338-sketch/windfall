//! Off-realtime private launch-channel authentication, before Load disclosure.

use std::{
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    process::Child,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
#[cfg(windows)]
use windows_sys::Win32::Security::Cryptography::{
    BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom,
};

const MAGIC: &[u8; 4] = b"WFAH";
// Hello4 commits to ABI10 project expression/ramp payloads before Load disclosure.
const VERSION: u32 = 4;
const HELLO_BYTES: usize = 40;
const BOOT_MAGIC: &[u8; 4] = b"WFAP";
const BOOT_VERSION: u32 = 1;
const BOOT_BYTES: usize = 56;
const MAX_STARTUP_MS: u64 = 60_000;
const CANDIDATES: usize = 8;
const CANDIDATE_TIMEOUT: Duration = Duration::from_millis(50);

pub(crate) fn check_startup_timeout(timeout: Duration) -> io::Result<()> {
    if timeout.is_zero() || timeout > Duration::from_millis(MAX_STARTUP_MS) {
        return Err(io::Error::other(
            "unsupported helper startup budget (positive, at most60s)",
        ));
    }
    Ok(())
}
fn ceil_millis(duration: Duration) -> io::Result<u64> {
    let millis = duration
        .as_nanos()
        .checked_add(999_999)
        .ok_or_else(|| io::Error::other("startup budget overflow"))?
        / 1_000_000;
    u64::try_from(millis).map_err(|_| io::Error::other("startup budget overflow"))
}
fn remaining_millis(deadline: Instant, now: Instant) -> io::Result<u64> {
    let remaining = deadline
        .checked_duration_since(now)
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::TimedOut,
                "audio helper startup deadline exceeded",
            )
        })?;
    let millis = ceil_millis(remaining)?;
    if millis == 0 || millis > MAX_STARTUP_MS {
        return Err(io::Error::other("unsupported remaining startup budget"));
    }
    Ok(millis)
}
fn parent_budget(deadline: Instant, cancelled: Option<&AtomicBool>) -> io::Result<u64> {
    if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err(io::Error::other("audio helper startup cancelled"));
    }
    remaining_millis(deadline, Instant::now())
}

pub struct Key([u8; 32]);
/// Export DLLs each have their own Rust statics. A PID/serial pair alone can
/// collide when a host loads two exported bundles/formats in the same process.
/// Generate an independent public namespace nonce off audio; it is separate
/// from (and never reveals) the subsequently generated bootstrap secret.
pub fn session_nonce() -> io::Result<u64> {
    let random = Key::generate()?;
    let nonce = u64::from_le_bytes(random.0[..8].try_into().expect("eight RNG bytes"));
    if nonce == 0 {
        return Err(io::Error::other("bridge namespace RNG returned zero"));
    }
    Ok(nonce)
}
impl Key {
    pub fn generate() -> io::Result<Self> {
        let mut bytes = [0; 32];
        #[cfg(windows)]
        {
            // SAFETY: sized writable buffer; system-preferred RNG needs no provider.
            let result = unsafe {
                BCryptGenRandom(
                    std::ptr::null_mut(),
                    bytes.as_mut_ptr(),
                    bytes.len() as u32,
                    BCRYPT_USE_SYSTEM_PREFERRED_RNG,
                )
            };
            if result != 0 {
                return Err(io::Error::other("bridge launch RNG failed"));
            }
        }
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
        Ok(Self(bytes))
    }
    pub fn write_to_child(
        &self,
        child: &mut Child,
        session: u64,
        deadline: Instant,
        cancelled: Option<&AtomicBool>,
    ) -> io::Result<()> {
        parent_budget(deadline, cancelled)?;
        // Fresh empty private pipe; the entire56B record fits one bounded write.
        // No key/session/budget in argv/environment/logs. Close after transfer.
        let mut pipe = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("helper needs private stdin"))?;
        self.write_bootstrap(&mut pipe, session, deadline, cancelled)
    }
    fn write_bootstrap(
        &self,
        pipe: &mut impl Write,
        session: u64,
        deadline: Instant,
        cancelled: Option<&AtomicBool>,
    ) -> io::Result<()> {
        if session == 0 {
            return Err(io::Error::other("invalid launch session"));
        }
        let millis = parent_budget(deadline, cancelled)?; // At actual pipe write.
        let mut record = [0u8; BOOT_BYTES];
        record[..4].copy_from_slice(BOOT_MAGIC);
        record[4..8].copy_from_slice(&BOOT_VERSION.to_le_bytes());
        record[8..40].copy_from_slice(&self.0);
        record[40..48].copy_from_slice(&session.to_le_bytes());
        record[48..56].copy_from_slice(&millis.to_le_bytes());
        let result = pipe.write_all(&record);
        record.fill(0);
        result?;
        parent_budget(deadline, cancelled)?;
        Ok(())
    }
    fn matches(&self, hello: &[u8; HELLO_BYTES]) -> bool {
        &hello[..4] == MAGIC
            && hello[4..8] == VERSION.to_le_bytes()
            && hello[8..]
                .iter()
                .zip(&self.0)
                .fold(0u8, |difference, (a, b)| difference | (a ^ b))
                == 0
    }
}
impl Drop for Key {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

/// Private owner bootstrap. Parent's absolute deadline remains authoritative;
/// the rounded remaining budget only bounds child connection/Hello/Load IO.
pub(crate) struct Bootstrap {
    key: Key,
    pub session: u64,
    pub deadline: Instant,
}
impl Bootstrap {
    pub fn read(input: &mut impl Read) -> io::Result<Self> {
        let mut record = [0u8; BOOT_BYTES];
        let result = input
            .read_exact(&mut record)
            .and_then(|()| Self::decode(&record, Instant::now()));
        record.fill(0);
        result
    }
    fn decode(record: &[u8; BOOT_BYTES], now: Instant) -> io::Result<Self> {
        if &record[..4] != BOOT_MAGIC || record[4..8] != BOOT_VERSION.to_le_bytes() {
            return Err(io::Error::other("unsupported private helper bootstrap"));
        }
        let session = u64::from_le_bytes(record[40..48].try_into().expect("bootstrap session"));
        let millis = u64::from_le_bytes(record[48..56].try_into().expect("bootstrap budget"));
        if session == 0 || millis == 0 || millis > MAX_STARTUP_MS {
            return Err(io::Error::other("invalid private helper bootstrap"));
        }
        let deadline = now
            .checked_add(Duration::from_millis(millis))
            .ok_or_else(|| io::Error::other("helper receive deadline overflow"))?;
        let mut key = Key([0; 32]);
        key.0.copy_from_slice(&record[8..40]);
        Ok(Self {
            key,
            session,
            deadline,
        })
    }
    pub fn remaining(&self) -> io::Result<Duration> {
        remaining_millis(self.deadline, Instant::now()).map(Duration::from_millis)
    }
    pub fn hello(&self, socket: &mut TcpStream) -> io::Result<()> {
        socket.set_write_timeout(Some(self.remaining()?))?;
        let mut hello = [0; HELLO_BYTES];
        hello[..4].copy_from_slice(MAGIC);
        hello[4..8].copy_from_slice(&VERSION.to_le_bytes());
        hello[8..].copy_from_slice(&self.key.0);
        let result = socket.write_all(&hello);
        hello.fill(0);
        result?;
        self.remaining()?;
        Ok(())
    }
}

struct Candidate {
    socket: TcpStream,
    bytes: [u8; HELLO_BYTES],
    received: usize,
    started: Instant,
}
/// Sends no bytes. The caller may disclose Load only on the returned socket.
/// Multiple bounded nonblocking candidates prevent a stalled first socket from
/// monopolizing startup. This is not protection from a privileged local actor.
pub fn accept(
    listener: &TcpListener,
    child: &mut Child,
    key: &Key,
    deadline: Instant,
    cancelled: Option<&AtomicBool>,
) -> io::Result<TcpStream> {
    let mut candidates: Vec<Candidate> = Vec::with_capacity(CANDIDATES);
    loop {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err(io::Error::other("helper authentication cancelled"));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "helper authentication timed out (requires Hello4 / mapping ABI10; incompatible helpers are refused)",
            ));
        }
        if child.try_wait()?.is_some() {
            return Err(io::Error::other("helper exited before authentication (requires Hello4 / mapping ABI10; incompatible helpers are refused)"));
        }
        // At most eight new accepts per iteration, including rejected excess.
        for _ in 0..CANDIDATES {
            match listener.accept() {
                Ok((socket, address)) => {
                    if address.ip().is_loopback() {
                        if candidates.len() == CANDIDATES {
                            // A full stalled set cannot exclude the newly
                            // launched child. Evict the oldest without writing.
                            let oldest = candidates
                                .iter()
                                .enumerate()
                                .min_by_key(|(_, candidate)| candidate.started)
                                .map(|(index, _)| index)
                                .expect("full candidate set");
                            candidates.swap_remove(oldest);
                        }
                        socket.set_nonblocking(true)?;
                        candidates.push(Candidate {
                            socket,
                            bytes: [0; HELLO_BYTES],
                            received: 0,
                            started: Instant::now(),
                        });
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => return Err(error),
            }
        }
        let mut at = 0;
        while at < candidates.len() {
            let candidate = &mut candidates[at];
            let mut reject = candidate.started.elapsed() >= CANDIDATE_TIMEOUT;
            if !reject {
                match candidate
                    .socket
                    .read(&mut candidate.bytes[candidate.received..])
                {
                    Ok(0) => reject = true,
                    Ok(length) => candidate.received += length,
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                        ) => {}
                    Err(_) => reject = true,
                }
            }
            if candidate.received >= 8
                && (&candidate.bytes[..4] != MAGIC
                    || candidate.bytes[4..8] != VERSION.to_le_bytes())
            {
                if candidate.received == HELLO_BYTES
                    && &candidate.bytes[..4] == MAGIC
                    && candidate.bytes[8..].iter().zip(&key.0)
                        .fold(0u8, |difference, (left, right)| difference | (left ^ right)) == 0
                {
                    return Err(io::Error::new(io::ErrorKind::Unsupported,
                        "audio helper protocol mismatch: this host requires Hello4 / mapping ABI10; replace the older helper"));
                }
                reject = true;
            }
            if candidate.received == HELLO_BYTES && !reject {
                if key.matches(&candidate.bytes) {
                    return Ok(candidates.swap_remove(at).socket);
                }
                reject = true;
            }
            if reject {
                candidates.swap_remove(at);
            } else {
                at += 1;
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_budget_validates_cap_rounding_and_expiration_without_clamping() {
        assert!(check_startup_timeout(Duration::ZERO).is_err());
        assert!(check_startup_timeout(Duration::from_nanos(1)).is_ok());
        assert!(check_startup_timeout(Duration::from_secs(60)).is_ok());
        assert!(check_startup_timeout(Duration::from_secs(60) + Duration::from_nanos(1)).is_err());
        assert!(check_startup_timeout(Duration::MAX).is_err());
        assert!(ceil_millis(Duration::MAX).is_err());
        let now = Instant::now();
        assert!(remaining_millis(now, now).is_err());
        assert!(remaining_millis(now - Duration::from_millis(1), now).is_err());
        assert_eq!(
            remaining_millis(now + Duration::from_nanos(1), now).unwrap(),
            1
        );
        assert_eq!(
            remaining_millis(now + Duration::from_secs(60), now).unwrap(),
            60_000
        );
        assert!(
            remaining_millis(now + Duration::from_secs(60) + Duration::from_nanos(1), now).is_err()
        );
    }
    #[test]
    fn private_bootstrap_is_exact_versioned_bounded_and_expired_writes_send_nothing() {
        let key = Key([42; 32]);
        let now = Instant::now();
        let mut bytes = Vec::new();
        key.write_bootstrap(&mut bytes, 7, now + Duration::from_secs(2), None)
            .unwrap();
        assert_eq!(bytes.len(), BOOT_BYTES);
        assert_eq!(&bytes[..4], BOOT_MAGIC);
        assert_eq!(&bytes[4..8], &BOOT_VERSION.to_le_bytes());
        let record: [u8; BOOT_BYTES] = bytes.as_slice().try_into().unwrap();
        let decoded = Bootstrap::decode(&record, now).unwrap();
        assert_eq!(decoded.session, 7);
        assert_eq!(decoded.key.0, key.0);
        assert!(decoded.deadline > now && decoded.deadline <= now + Duration::from_secs(2));
        for millis in [0, 60_001, u64::MAX] {
            let mut bad = record;
            bad[48..56].copy_from_slice(&millis.to_le_bytes());
            assert!(Bootstrap::decode(&bad, now).is_err());
        }
        let mut exact = record;
        exact[48..56].copy_from_slice(&60_000u64.to_le_bytes());
        assert_eq!(
            Bootstrap::decode(&exact, now).unwrap().deadline,
            now + Duration::from_secs(60)
        );
        let mut bad = record;
        bad[4..8].copy_from_slice(&2u32.to_le_bytes());
        assert!(Bootstrap::decode(&bad, now).is_err());
        let mut bad = record;
        bad[40..48].fill(0);
        assert!(Bootstrap::decode(&bad, now).is_err());
        let mut old = std::io::Cursor::new([0u8; 40]);
        assert_eq!(
            Bootstrap::read(&mut old).err().unwrap().kind(),
            io::ErrorKind::UnexpectedEof
        );
        let mut sent = Vec::new();
        assert_eq!(
            key.write_bootstrap(&mut sent, 7, now - Duration::from_millis(1), None)
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(sent.is_empty());
        let cancelled = AtomicBool::new(true);
        assert!(
            key.write_bootstrap(&mut sent, 7, now + Duration::from_secs(1), Some(&cancelled))
                .is_err()
        );
        assert!(sent.is_empty());
    }
    #[test]
    fn hello_has_exact_version_and_private_key_without_downgrade() {
        let key = Key([42; 32]);
        let mut hello = [0u8; HELLO_BYTES];
        hello[..4].copy_from_slice(MAGIC);
        hello[4..8].copy_from_slice(&VERSION.to_le_bytes());
        hello[8..].fill(42);
        assert!(key.matches(&hello));
        for index in 0..HELLO_BYTES {
            hello[index] ^= 1;
            assert!(!key.matches(&hello));
            hello[index] ^= 1;
        }
        hello[4..8].copy_from_slice(&0u32.to_le_bytes());
        assert!(!key.matches(&hello));
        hello[4..8].copy_from_slice(&1u32.to_le_bytes());
        assert!(!key.matches(&hello)); // Old helper cannot disclose/receive Load.
        hello[4..8].copy_from_slice(&2u32.to_le_bytes());
        assert!(!key.matches(&hello)); // ABI3 helper must not receive Load either.
        hello[4..8].copy_from_slice(&3u32.to_le_bytes());
        assert!(!key.matches(&hello)); // Older project-expression payloads refused.
    }
}
