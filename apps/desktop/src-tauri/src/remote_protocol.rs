//! Small bounded HTTP/WebSocket subset: one request, no chunking, no extensions
//! or fragmented/binary frames. This module never runs on an audio thread.
use std::io::{Read, Write};
use std::net::TcpStream;
pub const MAX_MESSAGE: usize = 4096;
pub const MAX_HEADER: usize = 8192;
pub struct Deadline<'a> {
    stream: &'a mut TcpStream,
    started: std::time::Instant,
}
impl<'a> Deadline<'a> {
    pub fn new(stream: &'a mut TcpStream) -> Self {
        Self {
            stream,
            started: std::time::Instant::now(),
        }
    }
}
impl Read for Deadline<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if self.started.elapsed() > std::time::Duration::from_secs(2) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Remote read deadline exceeded",
            ));
        }
        self.stream.read(bytes)
    }
}

pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: std::collections::BTreeMap<String, String>,
    pub body: Vec<u8>,
}
pub fn request(stream: &mut TcpStream) -> Result<Request, String> {
    let mut input = Deadline::new(stream);
    let mut raw = Vec::new();
    let started = std::time::Instant::now();
    loop {
        if started.elapsed() > std::time::Duration::from_secs(2) {
            return Err("HTTP request deadline exceeded".into());
        }
        if raw.len() >= MAX_HEADER {
            return Err("HTTP headers exceed limit".into());
        }
        let mut byte = [0];
        input.read_exact(&mut byte).map_err(err)?;
        raw.push(byte[0]);
        if raw.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let header = std::str::from_utf8(&raw).map_err(err)?;
    let mut lines = header.split("\r\n");
    let parts: Vec<_> = lines.next().ok_or("Missing request")?.split(' ').collect();
    if parts.len() != 3 || parts[2] != "HTTP/1.1" {
        return Err("Invalid HTTP request".into());
    }
    let mut headers = std::collections::BTreeMap::new();
    for line in lines.take_while(|line| !line.is_empty()) {
        let (key, value) = line.split_once(':').ok_or("Invalid HTTP header")?;
        if key.trim() != key || !key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-') {
            return Err("Invalid header name".into());
        }
        if headers
            .insert(key.to_ascii_lowercase(), value.trim().to_owned())
            .is_some()
        {
            return Err("Duplicate HTTP header".into());
        }
    }
    if headers.contains_key("transfer-encoding") {
        return Err("Chunking is unsupported".into());
    }
    let size = headers
        .get("content-length")
        .map(|n| n.parse::<usize>())
        .transpose()
        .map_err(err)?
        .unwrap_or(0);
    if size > MAX_MESSAGE {
        return Err("HTTP body exceeds limit".into());
    }
    let mut body = vec![0; size];
    input.read_exact(&mut body).map_err(err)?;
    Ok(Request {
        method: parts[0].into(),
        path: parts[1].into(),
        headers,
        body,
    })
}
pub fn response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> Result<(), String> {
    write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'\r\nConnection: close\r\n\r\n", body.len()).map_err(err)?;
    stream.write_all(body).map_err(err)
}
pub fn upgrade(stream: &mut TcpStream, request: &Request) -> Result<(), String> {
    if request
        .headers
        .get("upgrade")
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
        != Some("websocket")
        || !request.headers.get("connection").is_some_and(|s| {
            s.split(',')
                .any(|s| s.trim().eq_ignore_ascii_case("upgrade"))
        })
        || request
            .headers
            .get("sec-websocket-version")
            .map(String::as_str)
            != Some("13")
        || !request.body.is_empty()
    {
        return Err("Invalid WebSocket upgrade".into());
    }
    let key = request
        .headers
        .get("sec-websocket-key")
        .ok_or("Missing WebSocket key")?;
    if key.len() != 24
        || !key.ends_with("==")
        || !key[..22]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
    {
        return Err("Invalid WebSocket key".into());
    }
    let accept = websocket_accept(key);
    write!(stream, "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n").map_err(err)
}
pub fn read_frame(input: &mut impl Read) -> Result<Option<String>, String> {
    let mut head = [0; 2];
    input.read_exact(&mut head).map_err(err)?;
    if head[0] & 0x70 != 0 || head[0] & 0x80 == 0 || head[1] & 0x80 == 0 {
        return Err("Unmasked, extended or fragmented frame refused".into());
    }
    let opcode = head[0] & 15;
    if opcode != 1 && opcode != 8 {
        return Err("Only text and close frames are supported".into());
    }
    let size = match head[1] & 127 {
        126 => {
            let mut n = [0; 2];
            input.read_exact(&mut n).map_err(err)?;
            usize::from(u16::from_be_bytes(n))
        }
        127 => return Err("Frame exceeds limit".into()),
        n => usize::from(n),
    };
    if size > MAX_MESSAGE || (opcode == 8 && size > 125) {
        return Err("Frame exceeds limit".into());
    }
    let mut mask = [0; 4];
    input.read_exact(&mut mask).map_err(err)?;
    let mut bytes = vec![0; size];
    input.read_exact(&mut bytes).map_err(err)?;
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte ^= mask[i % 4];
    }
    if opcode == 8 {
        return Ok(None);
    }
    String::from_utf8(bytes).map(Some).map_err(err)
}
pub fn text_frame(out: &mut impl Write, text: &str) -> Result<(), String> {
    if text.len() > 65535 {
        return Err("Remote response exceeds limit".into());
    }
    out.write_all(&[0x81]).map_err(err)?;
    if text.len() < 126 {
        out.write_all(&[text.len() as u8]).map_err(err)?;
    } else {
        out.write_all(&[126]).map_err(err)?;
        out.write_all(&(text.len() as u16).to_be_bytes())
            .map_err(err)?;
    }
    out.write_all(text.as_bytes()).map_err(err)
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn websocket_accept(key: &str) -> String {
    // SHA-1 is used only for RFC6455 upgrade, never authentication/checksums.
    let mut bytes = format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11").into_bytes();
    let bits = bytes.len() as u64 * 8;
    bytes.push(128);
    while bytes.len() % 64 != 56 {
        bytes.push(0);
    }
    bytes.extend(bits.to_be_bytes());
    let mut h = [
        0x67452301u32,
        0xefcdab89,
        0x98badcfe,
        0x10325476,
        0xc3d2e1f0,
    ];
    for chunk in bytes.chunks_exact(64) {
        let mut w = [0u32; 80];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes(word.try_into().expect("four bytes"));
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5a827999u32),
                20..=39 => (b ^ c ^ d, 0x6ed9eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
                _ => (b ^ c ^ d, 0xca62c1d6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (out, value) in h.iter_mut().zip([a, b, c, d, e]) {
            *out = out.wrapping_add(value);
        }
    }
    let digest: Vec<_> = h.iter().flat_map(|word| word.to_be_bytes()).collect();
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in digest.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(alphabet[((n >> 18) & 63) as usize] as char);
        out.push(alphabet[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            alphabet[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            alphabet[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rfc6455_handshake_reference() {
        assert_eq!(
            websocket_accept("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }
    #[test]
    fn masked_text_and_limits() {
        assert_eq!(
            read_frame(&mut &b"\x81\x82\x00\x00\x00\x00ok"[..]).unwrap(),
            Some("ok".into())
        );
        assert!(read_frame(&mut &b"\x81\x7f"[..]).is_err());
        assert!(read_frame(&mut &b"\x01\x80"[..]).is_err());
        assert!(read_frame(&mut &b"\x81\xfe\x20\x00"[..]).is_err());
        assert!(read_frame(&mut &b"\x82\x80"[..]).is_err());
    }
}
