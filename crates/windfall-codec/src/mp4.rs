//! Checked inspection of the AAC-LC MP4 layout emitted by M4aWriter. This is
//! deliberately a narrow parser, not a general video/fragmented MP4 demuxer.
use std::ops::Range;
use crate::{AacConfig, AacTrim, CodecError, MAX_AAC_PACKET_BYTES, MAX_M4A_PACKETS};

#[derive(Debug, Clone)]
pub struct M4aInfo {
    pub config: AacConfig,
    pub trim: AacTrim,
    pub audio_specific_config: Vec<u8>,
    pub packet_ranges: Vec<Range<usize>>,
}
impl M4aInfo {
    pub fn frames(&self) -> u64 { (self.packet_ranges.len() as u64 * 1024).saturating_sub(u64::from(self.trim.delay) + u64::from(self.trim.padding)) }
}
fn bad() -> CodecError { CodecError::Corrupt("M4A: damaged or unsupported MP4 sample tables".into()) }
fn be(b: &[u8], at: usize) -> Result<u32, CodecError> {
    let v = b.get(at..at.checked_add(4).ok_or_else(bad)?).ok_or_else(bad)?;
    Ok(u32::from_be_bytes([v[0], v[1], v[2], v[3]]))
}
fn boxes(b: &[u8]) -> Result<Vec<([u8; 4], &[u8])>, CodecError> {
    let mut out = Vec::new(); let mut at = 0;
    while at < b.len() {
        if out.len() >= 256 { return Err(bad()); }
        let n = be(b, at)? as usize;
        if n < 8 || n > b.len() - at { return Err(bad()); }
        out.push(([b[at + 4], b[at + 5], b[at + 6], b[at + 7]], &b[at + 8..at + n])); at += n;
    }
    Ok(out)
}
fn child<'a>(b: &'a [u8], id: &[u8; 4]) -> Result<&'a [u8], CodecError> {
    let mut found = None;
    for (tag, body) in boxes(b)? {
        if &tag == id { if found.is_some() { return Err(bad()); } found = Some(body); }
    }
    found.ok_or_else(bad)
}
fn descriptor<'a>(b: &'a [u8], at: &mut usize, tag: u8) -> Result<&'a [u8], CodecError> {
    if b.get(*at) != Some(&tag) { return Err(bad()); } *at += 1;
    let mut size = 0;
    for i in 0..4 {
        let byte = *b.get(*at).ok_or_else(bad)?; *at += 1;
        size = (size << 7) | usize::from(byte & 127);
        if byte & 128 == 0 {
            let end = (*at).checked_add(size).ok_or_else(bad)?;
            let data = b.get(*at..end).ok_or_else(bad)?; *at = end; return Ok(data);
        }
        if i == 3 { return Err(bad()); }
    }
    Err(bad())
}
pub fn inspect_m4a(bytes: &[u8]) -> Result<M4aInfo, CodecError> {
    let ftyp = child(bytes, b"ftyp")?;
    if !ftyp.starts_with(b"M4A ") { return Err(bad()); }
    let mdat = child(bytes, b"mdat")?;
    let media_start = mdat.as_ptr() as usize - bytes.as_ptr() as usize;
    let moov = child(bytes, b"moov")?;
    let trak = child(moov, b"trak")?;
    let mdia = child(trak, b"mdia")?;
    let stbl = child(child(mdia, b"minf")?, b"stbl")?;
    let stsd = child(stbl, b"stsd")?;
    if be(stsd, 0)? != 0 || be(stsd, 4)? != 1 { return Err(bad()); }
    let entry = child(stsd.get(8..).ok_or_else(bad)?, b"mp4a")?;
    if entry.len() < 28 { return Err(bad()); }
    let channels = u16::from_be_bytes([entry[16], entry[17]]);
    let sample_rate = be(entry, 24)? >> 16;
    if ![1, 2].contains(&channels) || ![44100, 48000, 64000, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350].contains(&sample_rate) { return Err(bad()); }
    let config = AacConfig { sample_rate, channels };
    let esds = child(&entry[28..], b"esds")?;
    if be(esds, 0)? != 0 { return Err(bad()); }
    let mut at = 4; let es = descriptor(esds, &mut at, 3)?;
    if es.len() < 3 || es[2] != 0 { return Err(bad()); }
    let mut at = 3; let decoder = descriptor(es, &mut at, 4)?;
    if decoder.len() < 13 || decoder[0] != 0x40 || decoder[1] != 0x15 { return Err(bad()); }
    let mut at = 13; let asc = descriptor(decoder, &mut at, 5)?;
    if asc.len() != 2 { return Err(bad()); }
    let bits = u16::from_be_bytes([asc[0], asc[1]]);
    let rates = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350];
    if bits >> 11 != 2 || rates.get(((bits >> 7) & 15) as usize) != Some(&sample_rate) || (bits >> 3) & 15 != channels || bits & 7 != 0 { return Err(bad()); }
    let sz = child(stbl, b"stsz")?;
    let count = be(sz, 8)? as usize;
    if count == 0 || count > MAX_M4A_PACKETS || be(sz, 0)? != 0 || be(sz, 4)? != 0 || sz.len() != 12 + count * 4 { return Err(bad()); }
    let stts = child(stbl, b"stts")?;
    if stts.len() != 16 || be(stts, 0)? != 0 || be(stts, 4)? != 1 || be(stts, 8)? as usize != count || be(stts, 12)? != 1024 { return Err(bad()); }
    let stco = child(stbl, b"stco")?;
    if stco.len() != 12 || be(stco, 4)? != 1 || be(stco, 8)? as usize != media_start { return Err(bad()); }
    let stsc = child(stbl, b"stsc")?;
    if stsc.len() != 20 || be(stsc, 4)? != 1 || be(stsc, 8)? != 1 || be(stsc, 12)? as usize != count || be(stsc, 16)? != 1 { return Err(bad()); }
    let mdhd = child(mdia, b"mdhd")?;
    let duration = count as u64 * 1024;
    if be(mdhd, 0)? != 0 || be(mdhd, 12)? != sample_rate || u64::from(be(mdhd, 16)?) != duration { return Err(bad()); }
    let edit = child(child(trak, b"edts")?, b"elst")?;
    if edit.len() != 20 || be(edit, 0)? != 0 || be(edit, 4)? != 1 || be(edit, 16)? != 0x10000 { return Err(bad()); }
    let playable = u64::from(be(edit, 8)?); let delay = be(edit, 12)?;
    if playable == 0 || playable + u64::from(delay) > duration { return Err(bad()); }
    let mvhd = child(moov, b"mvhd")?;
    if be(mvhd, 12)? != sample_rate || u64::from(be(mvhd, 16)?) != playable { return Err(bad()); }
    let trim = AacTrim { delay, padding: (duration - playable - u64::from(delay)) as u32 };
    let mut packet_ranges = Vec::new();
    packet_ranges.try_reserve_exact(count).map_err(|_| CodecError::OutOfMemory)?;
    let mut offset = media_start;
    for i in 0..count {
        let n = be(sz, 12 + i * 4)? as usize;
        if n == 0 || n > MAX_AAC_PACKET_BYTES || n > media_start + mdat.len() - offset { return Err(bad()); }
        packet_ranges.push(offset..offset + n); offset += n;
    }
    if offset != media_start + mdat.len() { return Err(bad()); }
    Ok(M4aInfo { config, trim, audio_specific_config: asc.to_vec(), packet_ranges })
}
