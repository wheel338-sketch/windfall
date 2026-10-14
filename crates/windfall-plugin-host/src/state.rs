//! A plugin's saved state: the bytes a project stores to bring a plugin
//! back exactly as it was.
//!
//! The host does not look inside. What a plugin writes is the plugin's
//! business, and the same bytes go back to it when the project opens. The
//! host only wraps them in a few bytes of its own that say what they are,
//! because a plugin without a state extension is saved as its parameter
//! values instead and the two must not be confused.

use std::io::{self, Write};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// What every state starts with.
const MAGIC: &[u8; 4] = b"WFPS";
const VERSION: u8 = 1;
const KIND_NATIVE: u8 = 0;
const KIND_PARAMETERS: u8 = 1;
const HEADER: usize = MAGIC.len() + 2;

/// The largest state the host accepts from a plugin, 256 MiB. A sampler
/// that embeds its samples comes nowhere near, and a plugin that writes
/// without end is stopped before memory runs out.
pub const MAX_STATE_BYTES: usize = 256 << 20;

/// The saved state of one plugin instance.
///
/// In JSON it is a base64 string, so a project file can hold it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginState {
    bytes: Vec<u8>,
}

/// What a state holds, once its header has been read.
#[derive(Debug, PartialEq)]
pub(crate) enum StateContent<'a> {
    /// The bytes the plugin's own state extension wrote.
    Native(&'a [u8]),
    /// The value of every parameter, by id, for a plugin that cannot save
    /// itself.
    Parameters(Vec<(u32, f64)>),
}

/// The bytes are not a state this version of the host wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{}", windfall_core::notice!("pluginHost.invalidState"))]
pub struct InvalidState;

impl PluginState {
    /// Wraps bytes that [`PluginState::as_bytes`] returned earlier.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// The whole state, header included, as stored.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    fn with_header(kind: u8, payload_len: usize) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HEADER + payload_len);
        bytes.extend_from_slice(MAGIC);
        bytes.push(VERSION);
        bytes.push(kind);
        bytes
    }

    /// A state around what a plugin's state extension wrote.
    pub(crate) fn native(payload: &[u8]) -> Self {
        let mut bytes = Self::with_header(KIND_NATIVE, payload.len());
        bytes.extend_from_slice(payload);
        Self { bytes }
    }

    /// A state that is a list of parameter values.
    pub(crate) fn parameters(values: &[(u32, f64)]) -> Self {
        let mut bytes = Self::with_header(KIND_PARAMETERS, 4 + values.len() * 12);
        bytes.extend_from_slice(&(values.len() as u32).to_le_bytes());
        for (id, value) in values {
            bytes.extend_from_slice(&id.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        Self { bytes }
    }

    pub(crate) fn content(&self) -> Result<StateContent<'_>, InvalidState> {
        let (header, payload) = self.bytes.split_at_checked(HEADER).ok_or(InvalidState)?;
        if &header[..4] != MAGIC || header[4] != VERSION {
            return Err(InvalidState);
        }
        match header[5] {
            KIND_NATIVE => Ok(StateContent::Native(payload)),
            KIND_PARAMETERS => {
                let (count, pairs) = payload.split_at_checked(4).ok_or(InvalidState)?;
                let count = u32::from_le_bytes(count.try_into().expect("four bytes")) as usize;
                if pairs.len() != count.checked_mul(12).ok_or(InvalidState)? {
                    return Err(InvalidState);
                }
                let values = pairs
                    .as_chunks::<12>()
                    .0
                    .iter()
                    .map(|pair| {
                        let id = u32::from_le_bytes(pair[..4].try_into().expect("four bytes"));
                        let value = f64::from_le_bytes(pair[4..].try_into().expect("eight bytes"));
                        (id, value)
                    })
                    .collect();
                Ok(StateContent::Parameters(values))
            }
            _ => Err(InvalidState),
        }
    }
}

/// A writer that takes at most a set number of bytes and then fails, for
/// the stream a plugin saves into.
pub(crate) struct LimitedWriter {
    pub bytes: Vec<u8>,
    limit: usize,
    failed: bool,
}

impl LimitedWriter {
    pub fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            failed: false,
        }
    }
    pub fn failed(&self) -> bool {
        self.failed
    }
}

impl Write for LimitedWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.failed
            || self
                .bytes
                .len()
                .checked_add(data.len())
                .is_none_or(|size| size > self.limit)
        {
            self.failed = true;
            return Err(io::Error::other("the plugin's state is too large"));
        }
        self.bytes.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.failed {
            Err(io::Error::other("the plugin's state writer failed"))
        } else {
            Ok(())
        }
    }
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn to_base64(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for group in bytes.chunks(3) {
        let word = (u32::from(group[0]) << 16)
            | (u32::from(*group.get(1).unwrap_or(&0)) << 8)
            | u32::from(*group.get(2).unwrap_or(&0));
        for position in 0..4 {
            if position <= group.len() {
                let index = (word >> (18 - 6 * position)) & 0x3f;
                text.push(ALPHABET[index as usize] as char);
            } else {
                text.push('=');
            }
        }
    }
    text
}

fn from_base64(text: &str) -> Option<Vec<u8>> {
    let text = text.trim_end_matches('=').as_bytes();
    let mut bytes = Vec::with_capacity(text.len() * 3 / 4);
    for group in text.chunks(4) {
        if group.len() == 1 {
            return None;
        }
        let mut word = 0_u32;
        for (position, symbol) in group.iter().enumerate() {
            let index = ALPHABET.iter().position(|candidate| candidate == symbol)?;
            word |= (index as u32) << (18 - 6 * position);
        }
        let [_, first, second, third] = word.to_be_bytes();
        bytes.extend_from_slice(&[first, second, third][..group.len() - 1]);
    }
    Some(bytes)
}

impl Serialize for PluginState {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&to_base64(&self.bytes))
    }
}

impl<'de> Deserialize<'de> for PluginState {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        from_base64(&text)
            .map(Self::from_bytes)
            .ok_or_else(|| serde::de::Error::custom("a plugin state must be base64"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_and_reads_back() {
        let cases: [(&[u8], &str); 5] = [
            (b"", ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foobar", "Zm9vYmFy"),
        ];
        for (bytes, text) in cases {
            assert_eq!(to_base64(bytes), text);
            assert_eq!(from_base64(text).as_deref(), Some(bytes));
        }
        let every_byte: Vec<u8> = (0..=255).collect();
        assert_eq!(from_base64(&to_base64(&every_byte)), Some(every_byte));
        assert_eq!(from_base64("Zm9v!"), None);
    }

    #[test]
    fn a_state_survives_json_and_says_what_it_holds() {
        let native = PluginState::native(&[1, 2, 3, 250]);
        let json = serde_json::to_string(&native).unwrap();
        let back: PluginState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, native);
        assert_eq!(back.content(), Ok(StateContent::Native(&[1, 2, 3, 250])));

        let values = [(7, 0.5), (9, 1.0)];
        let snapshot = PluginState::parameters(&values);
        assert_eq!(
            snapshot.content(),
            Ok(StateContent::Parameters(values.to_vec()))
        );
    }

    #[test]
    fn damaged_states_are_refused() {
        for bytes in [
            b"".to_vec(),
            b"WFPS".to_vec(),
            b"NOPE\x01\x00abc".to_vec(),
            b"WFPS\x02\x00abc".to_vec(),
            b"WFPS\x01\x07abc".to_vec(),
            b"WFPS\x01\x01\x02\x00\x00\x00short".to_vec(),
        ] {
            assert_eq!(PluginState::from_bytes(bytes).content(), Err(InvalidState));
        }
    }

    #[test]
    fn the_limited_writer_stops_at_its_limit() {
        let mut writer = LimitedWriter::new(4);
        assert!(writer.write_all(b"abcd").is_ok());
        assert!(writer.write_all(b"e").is_err());
        assert_eq!(writer.bytes, b"abcd");
    }

    #[test]
    fn a_refused_state_write_poison_is_sticky() {
        let mut writer = LimitedWriter::new(4);
        writer.write_all(b"ab").unwrap();
        assert!(writer.write_all(b"cde").is_err());
        assert_eq!(writer.bytes, b"ab");
        assert!(writer.failed());
        assert!(writer.write_all(b"c").is_err());
        assert!(writer.flush().is_err());
        assert_eq!(writer.bytes, b"ab");
    }
}
