//! Native-only project packaging. No audio or document locks are used here.
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tempfile::{NamedTempFile, TempDir};
use thiserror::Error;
use windfall_project::{Project, ProjectSession, SampleId, SamplePath, file};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

#[cfg(test)]
mod tests;

pub const SCHEMA: u32 = 1;
const MANIFEST: &str = "manifest.json";
const PROJECT: &str = "project.windfall";
const CHUNK: usize = 64 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub entries: usize,
    pub expanded_bytes: u64,
    pub entry_bytes: u64,
    pub json_bytes: u64,
    pub archive_bytes: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            entries: 4096,
            expanded_bytes: 2 * 1024 * 1024 * 1024,
            entry_bytes: 1024 * 1024 * 1024,
            json_bytes: 8 * 1024 * 1024,
            archive_bytes: 2 * 1024 * 1024 * 1024,
        }
    }
}

/// Called between bounded IO chunks and immediately before publication/install.
/// A caller can cancel or inject an IO error without affecting other files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Reservation,
    Published,
    Source,
    Write,
    Publish,
    Read,
    Extract,
    Ready,
}
pub type Check<'a> = dyn FnMut(Stage) -> Result<(), Error> + 'a;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{}", windfall_core::notice!("archive.error.cancelled"))]
    Cancelled,
    #[error("{0}")]
    Invalid(String),
    #[error("{}", windfall_core::notice!("archive.error.missing", detail = windfall_core::notice::term(_0.to_string())))]
    Missing(String),
    #[error("{}", windfall_core::notice!("archive.error.io", detail = windfall_core::notice::term(_0.to_string())))]
    Io(#[from] io::Error),
    #[error("{}", windfall_core::notice!("archive.error.zip", detail = windfall_core::notice::term(_0.to_string())))]
    Zip(#[from] zip::result::ZipError),
    #[error("{}", windfall_core::notice!("archive.error.json", detail = windfall_core::notice::term(_0.to_string())))]
    Json(#[from] serde_json::Error),
}
fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub project: String,
    pub audio: Vec<Audio>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Audio {
    pub sample: SampleId,
    pub member: String,
    pub path: String,
    pub bytes: u64,
    pub original_name: String,
    pub original_path: SamplePath,
}

fn bounded(value: u64, limit: u64) -> Result<(), Error> {
    if value > limit {
        Err(invalid("Project archive exceeds its size limit."))
    } else {
        Ok(())
    }
}
fn copy<R: Read, W: Write>(
    from: &mut R,
    to: &mut W,
    limit: u64,
    stage: Stage,
    check: &mut Check<'_>,
) -> Result<(u64, u32), Error> {
    let mut buffer = [0; CHUNK];
    let mut bytes = 0u64;
    let mut hash = crc32fast::Hasher::new();
    loop {
        check(stage)?;
        let n = from.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes = bytes
            .checked_add(n as u64)
            .ok_or_else(|| invalid("Archive size overflow."))?;
        bounded(bytes, limit)?;
        hash.update(&buffer[..n]);
        to.write_all(&buffer[..n])?;
    }
    Ok((bytes, hash.finalize()))
}
fn equal(a: &Path, b: &Path, check: &mut Check<'_>) -> Result<bool, Error> {
    let mut a = File::open(a)?;
    let mut b = File::open(b)?;
    let mut x = [0; CHUNK];
    let mut y = [0; CHUNK];
    loop {
        check(Stage::Source)?;
        let n = a.read(&mut x)?;
        b.read_exact(&mut y[..n])?;
        if x[..n] != y[..n] {
            return Ok(false);
        }
        if n == 0 {
            return Ok(true);
        }
    }
}

/// Packages a private snapshot, deduplicating exact audio bytes (CRC is only a
/// candidate index). Publication uses a no-clobber atomic persist. Existing
/// targets are never replaced, including a competitor arriving during work.
pub fn create(
    project: &Project,
    session: Option<&ProjectSession>,
    project_dir: Option<&Path>,
    factory_dir: &Path,
    target: &Path,
    limits: Limits,
    check: &mut Check<'_>,
) -> Result<(), Error> {
    let mut owned = project.clone();
    owned.own_soundfont_sources().map_err(invalid)?;
    let project = &owned;
    project.check().map_err(invalid)?;
    if project.samples.len().saturating_add(2) > limits.entries {
        return Err(invalid("Too many archive audio sources."));
    }
    let sources: Vec<_> = project
        .samples
        .iter()
        .map(|sample| file::resolve_sample_path(&sample.path, project_dir, factory_dir))
        .collect();
    let missing: Vec<_> = project
        .samples
        .iter()
        .zip(&sources)
        .filter(|(_, path)| path.as_ref().is_none_or(|path| !path.is_file()))
        .map(|(sample, _)| format!("{} ({:?})", sample.name, sample.path))
        .collect();
    if !missing.is_empty() {
        return Err(Error::Missing(missing.join("\n")));
    }
    let parent = target
        .parent()
        .ok_or_else(|| invalid("Archive target has no folder."))?;
    // Caller chooses an existing destination. Only this request's temporary
    // directory and file are owned; failures never delete destination files.
    check(Stage::Reservation)?;
    let staging = tempfile::tempdir_in(parent)?;
    let mut snapshot = project.clone();
    let mut manifest = Manifest {
        schema: SCHEMA,
        project: PROJECT.into(),
        audio: Vec::new(),
    };
    let mut unique: Vec<(String, PathBuf, u64)> = Vec::new();
    let mut candidates: HashMap<(u64, u32), Vec<usize>> = HashMap::new();
    let mut expanded = 0u64;
    let mut staged_bytes = 0u64;
    for (index, (sample, source)) in snapshot.samples.iter_mut().zip(sources).enumerate() {
        let source = source.ok_or_else(|| invalid("Missing source."))?;
        let staged = staging.path().join(index.to_string());
        let remaining = limits
            .expanded_bytes
            .saturating_sub(staged_bytes)
            .min(limits.entry_bytes);
        let mut input = File::open(&source)?;
        let before = input.metadata()?;
        bounded(before.len(), remaining)?;
        let (bytes, crc) = copy(
            &mut input,
            &mut File::create(&staged)?,
            remaining,
            Stage::Source,
            check,
        )?;
        let after = input.metadata()?;
        if bytes != before.len()
            || before.len() != after.len()
            || before.modified().ok() != after.modified().ok()
        {
            return Err(invalid(
                windfall_core::notice!("archive.sourceChanged", file = source.display())
                    .to_string(),
            ));
        }
        staged_bytes += bytes;
        let mut found = None;
        for &candidate in candidates.get(&(bytes, crc)).into_iter().flatten() {
            if equal(&staged, &unique[candidate].1, check)? {
                found = Some(candidate);
                break;
            }
        }
        let member = if let Some(found) = found {
            unique[found].0.clone()
        } else {
            expanded = expanded
                .checked_add(bytes)
                .ok_or_else(|| invalid("Archive size overflow."))?;
            bounded(expanded, limits.expanded_bytes)?;
            let extension = source
                .extension()
                .and_then(|s| s.to_str())
                .filter(|s| s.len() <= 12 && s.bytes().all(|c| c.is_ascii_alphanumeric()))
                .unwrap_or("audio")
                .to_ascii_lowercase();
            let member = format!("audio/{:04}.{extension}", unique.len() + 1);
            candidates
                .entry((bytes, crc))
                .or_default()
                .push(unique.len());
            unique.push((member.clone(), staged, bytes));
            member
        };
        let extension = member.rsplit('.').next().unwrap_or("audio");
        let path = format!("samples/{:04}.{extension}", sample.id.0);
        manifest.audio.push(Audio {
            sample: sample.id,
            member,
            path: path.clone(),
            bytes,
            original_name: sample.name.clone(),
            original_path: sample.path.clone(),
        });
        sample.path = SamplePath::Project(path);
    }
    for channel in &mut snapshot.channels {
        if let Some(windfall_project::SampleInstrumentAsset::SoundFont {
            source: Some(id),
            path,
            ..
        }) = &mut channel.sample_instrument
        {
            *path = snapshot
                .samples
                .iter()
                .find(|s| s.id == *id)
                .ok_or_else(|| invalid("Missing SoundFont identity"))?
                .path
                .clone();
        }
    }
    let json = file::to_json_with(&snapshot, session)?;
    let manifest_json = serde_json::to_vec_pretty(&manifest)?;
    bounded(json.len() as u64, limits.json_bytes)?;
    bounded(manifest_json.len() as u64, limits.json_bytes)?;
    // Extraction holds unique members plus one regular sample file per ID.
    bounded(
        expanded + staged_bytes + json.len() as u64 + manifest_json.len() as u64,
        limits.expanded_bytes,
    )?;
    let mut temp = NamedTempFile::new_in(parent)?;
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    {
        let mut writer = ZipWriter::new(temp.as_file_mut());
        for (name, bytes) in [
            (MANIFEST, manifest_json.as_slice()),
            (PROJECT, json.as_bytes()),
        ] {
            check(Stage::Write)?;
            writer.start_file(name, options)?;
            writer.write_all(bytes)?;
        }
        for (member, source, bytes) in unique {
            writer.start_file(member, options)?;
            copy(
                &mut File::open(source)?,
                &mut writer,
                bytes,
                Stage::Write,
                check,
            )?;
        }
        writer.finish()?;
    }
    bounded(temp.as_file().metadata()?.len(), limits.archive_bytes)?;
    temp.as_file().sync_all()?;
    check(Stage::Publish)?;
    temp.persist_noclobber(target)
        .map_err(|error| error.error)?;
    check(Stage::Published)?;
    Ok(())
}

/// An extraction remains request-owned until installation succeeds. Keeping
/// this guard cleans rejected/stale/cancelled requests, never an unrelated path.
pub struct Extracted {
    // Drop the open Windows lock before TempDir tries to remove its directory.
    owner: File,
    root: TempDir,
    pub project: Project,
    pub session: Option<ProjectSession>,
}
impl Extracted {
    pub fn root(&self) -> &Path {
        self.root.path()
    }
    pub fn retain(self) -> PathBuf {
        self.root.keep()
    }
    /// Transfer the exclusive extraction lease with the directory. An owning
    /// session keeps this OS lock until its last clone ends; a crash releases it.
    pub fn retain_owned(self) -> (PathBuf, File) {
        (self.root.keep(), self.owner)
    }
}

/// Durable ownership begins before the first extraction byte. A collector
/// must acquire OWNER_LOCK and validate this marker before touching a root.
pub const OWNER_LOCK: &str = ".windfall-owner.lock";
pub const OWNERSHIP: &str = ".windfall-extraction.json";
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtractionRecord {
    pub version: u32,
    pub installed: bool,
    pub capture: bool,
    pub references: Vec<PathBuf>,
}

fn member_name(name: &str) -> Result<(), Error> {
    // Format members use ASCII generated names; Unicode source names are
    // metadata. This avoids filesystem normalization/case ambiguity on all OSes.
    if name.len() > 240
        || !name.is_ascii()
        || name.is_empty()
        || name.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.ends_with(['.', ' '])
                || part.bytes().any(|c| c < 32 || b"\\:<>\"|?*".contains(&c))
                || matches!(
                    part.split('.')
                        .next()
                        .unwrap_or("")
                        .to_ascii_uppercase()
                        .as_str(),
                    "CON"
                        | "PRN"
                        | "AUX"
                        | "NUL"
                        | "COM1"
                        | "COM2"
                        | "COM3"
                        | "COM4"
                        | "COM5"
                        | "COM6"
                        | "COM7"
                        | "COM8"
                        | "COM9"
                        | "LPT1"
                        | "LPT2"
                        | "LPT3"
                        | "LPT4"
                        | "LPT5"
                        | "LPT6"
                        | "LPT7"
                        | "LPT8"
                        | "LPT9"
                )
        })
    {
        return Err(invalid(
            windfall_core::notice!("archive.unsafeMember", name = format!("{:?}", name))
                .to_string(),
        ));
    }
    Ok(())
}
fn u16le(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}
fn u32le(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

fn zip32_extra(bytes: &[u8]) -> Result<(), Error> {
    let mut at = 0;
    while at < bytes.len() {
        if at + 4 > bytes.len() {
            return Err(invalid("Truncated ZIP extra field."));
        }
        let kind = u16le(bytes, at);
        at += 4 + u16le(bytes, at + 2) as usize;
        if at > bytes.len() || kind == 1 {
            return Err(invalid("Unsupported or damaged ZIP extra field."));
        }
    }
    Ok(())
}

fn local_span_end(start: u64, bytes: u64, directory_start: u64) -> Result<u64, Error> {
    start
        .checked_add(bytes)
        .filter(|end| *end <= directory_start)
        .ok_or_else(|| invalid("ZIP local record exceeds the archive data region."))
}

/// ZIP32 descriptors may carry a signature or start directly with the CRC.
/// Try the unsigned form first: a legitimate CRC can equal the signature.
fn descriptor_end<R: Read + Seek>(
    input: &mut R,
    start: u64,
    directory_start: u64,
    expected: [u32; 3],
) -> Result<u64, Error> {
    let short_end = local_span_end(start, 12, directory_start)?;
    input.seek(SeekFrom::Start(start))?;
    let mut bytes = [0; 16];
    input.read_exact(&mut bytes[..12])?;
    if [u32le(&bytes, 0), u32le(&bytes, 4), u32le(&bytes, 8)] == expected {
        return Ok(short_end);
    }
    if bytes[..4] == *b"PK\x07\x08" {
        let end = local_span_end(start, 16, directory_start)?;
        input.read_exact(&mut bytes[12..])?;
        if [u32le(&bytes, 4), u32le(&bytes, 8), u32le(&bytes, 12)] == expected {
            return Ok(end);
        }
    }
    Err(invalid("ZIP data descriptor does not match the directory."))
}

/// Bind ZIP's later local-header seek to the validated central identity.
/// Read only bounded header/extra/descriptor metadata; compressed payloads
/// remain streamed by the existing CRC-checked reader after initialization.
fn local_end<R: Read + Seek>(
    input: &mut R,
    name: &[u8],
    central: &[u8],
    directory_start: u64,
    check: &mut Check<'_>,
) -> Result<(u64, u64), Error> {
    let start = u32le(central, 42) as u64;
    let fixed_end = local_span_end(start, 30, directory_start)?;
    input.seek(SeekFrom::Start(start))?;
    let mut header = [0; 30];
    input.read_exact(&mut header)?;
    let flags = u16le(central, 8);
    if header[..4] != *b"PK\x03\x04"
        || u16le(&header, 6) != flags
        || u16le(&header, 8) != u16le(central, 10)
        || u16le(&header, 26) as usize != name.len()
    {
        return Err(invalid("ZIP local header does not match the directory."));
    }
    let extra_length = u16le(&header, 28) as usize;
    let data_start = local_span_end(
        fixed_end,
        name.len() as u64 + extra_length as u64,
        directory_start,
    )?;
    let expected = [u32le(central, 16), u32le(central, 20), u32le(central, 24)];
    let local = [u32le(&header, 14), u32le(&header, 18), u32le(&header, 22)];
    let descriptor = flags & 8 != 0;
    if (descriptor
        && local
            .iter()
            .zip(expected)
            .any(|(local, expected)| *local != 0 && *local != expected))
        || (!descriptor && local != expected)
        || (u16le(central, 10) == 0 && expected[1] != expected[2])
    {
        return Err(invalid(
            "ZIP local CRC or sizes do not match the directory.",
        ));
    }
    let mut local_name = vec![0; name.len()];
    check(Stage::Read)?;
    input.read_exact(&mut local_name)?;
    if local_name != name {
        return Err(invalid("ZIP local name does not match the directory."));
    }
    // Each allocation/read is bounded by the ZIP32 u16 extra length, and
    // names already passed the small schema-path bound.
    let mut extra = vec![0; extra_length];
    check(Stage::Read)?;
    input.read_exact(&mut extra)?;
    zip32_extra(&extra)?;
    let data_end = local_span_end(data_start, expected[1] as u64, directory_start)?;
    let end = if descriptor {
        check(Stage::Read)?;
        descriptor_end(input, data_end, directory_start, expected)?
    } else {
        data_end
    };
    Ok((start, end))
}

/// Bounds the central directory BEFORE ZIP's parser allocates its index, and
/// detects duplicates before any parser can coalesce them. ZIP64/multidisk,
/// encrypted members, directories and non-regular UNIX types are unsupported.
struct Preflight {
    members: HashMap<String, u64>,
    index: IndexReader,
}

fn preflight<R: Read + Seek>(
    input: &mut R,
    limits: Limits,
    check: &mut Check<'_>,
) -> Result<Preflight, Error> {
    let length = input.seek(SeekFrom::End(0))?;
    bounded(length, limits.archive_bytes)?;
    let tail_size = length.min(65557) as usize;
    input.seek(SeekFrom::End(-(tail_size as i64)))?;
    let mut tail = vec![0; tail_size];
    input.read_exact(&mut tail)?;
    let end = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&at| {
            tail[at..at + 4] == *b"PK\x05\x06"
                && at + 22 + u16le(&tail, at + 20) as usize == tail.len()
        })
        .ok_or_else(|| invalid("Missing ZIP directory."))?;
    let count = u16le(&tail, end + 10) as usize;
    let size = u32le(&tail, end + 12) as u64;
    let offset = u32le(&tail, end + 16) as u64;
    if u16le(&tail, end + 4) != 0
        || u16le(&tail, end + 6) != 0
        || u16le(&tail, end + 8) as usize != count
        || count == 65535
        || size == u32::MAX as u64
        || offset == u32::MAX as u64
        || count > limits.entries
        || size > 4 * 1024 * 1024
        || offset + size != length - tail_size as u64 + end as u64
    {
        return Err(invalid("Unsupported or oversized ZIP directory."));
    }
    if tail[end + 22..]
        .windows(4)
        .any(|magic| matches!(magic, b"PK\x05\x06" | b"PK\x06\x06" | b"PK\x06\x07"))
    {
        return Err(invalid("Ambiguous ZIP directory comment."));
    }
    input.seek(SeekFrom::Start(offset))?;
    let mut directory = vec![0; size as usize];
    input.read_exact(&mut directory)?;
    let mut at = 0;
    let mut names = HashMap::new();
    let mut expanded = 0u64;
    let mut ranges = Vec::with_capacity(count);
    for _ in 0..count {
        check(Stage::Read)?;
        if at + 46 > directory.len() || directory[at..at + 4] != *b"PK\x01\x02" {
            return Err(invalid("Damaged ZIP directory."));
        }
        let name_length = u16le(&directory, at + 28) as usize;
        let next = at
            + 46
            + name_length
            + u16le(&directory, at + 30) as usize
            + u16le(&directory, at + 32) as usize;
        if next > directory.len() {
            return Err(invalid("Truncated ZIP directory."));
        }
        let name = std::str::from_utf8(&directory[at + 46..at + 46 + name_length])
            .map_err(|_| invalid("Invalid archive path encoding."))?;
        member_name(name)?;
        let mode = u32le(&directory, at + 38) >> 16;
        let bytes = u32le(&directory, at + 24) as u64;
        // ZIP64 extras can override the sizes and local offset read above.
        // Refuse them before ZIP sees any metadata, including non-sentinel
        // fields, which ZIP 6 also permits ZIP64 extras to replace.
        let extra = at + 46 + name_length;
        let extra_end = extra + u16le(&directory, at + 30) as usize;
        zip32_extra(&directory[extra..extra_end])?;
        if u16le(&directory, at + 8) & 1 != 0
            || !matches!(u16le(&directory, at + 10), 0 | 8)
            || u16le(&directory, at + 34) != 0
            || !matches!(mode & 0o170000, 0 | 0o100000)
            || u32le(&directory, at + 38) & 0x10 != 0
            || bytes == u32::MAX as u64
            || u32le(&directory, at + 20) == u32::MAX
            || u32le(&directory, at + 42) == u32::MAX
        {
            return Err(invalid(
                "Unsupported encrypted, linked or special ZIP member.",
            ));
        }
        bounded(bytes, limits.entry_bytes)?;
        expanded = expanded
            .checked_add(bytes)
            .ok_or_else(|| invalid("Archive size overflow."))?;
        bounded(expanded, limits.expanded_bytes)?;
        if names.insert(name.to_ascii_lowercase(), bytes).is_some() {
            return Err(invalid("Duplicate or case-colliding ZIP members."));
        }
        ranges.push(local_end(
            input,
            name.as_bytes(),
            &directory[at..at + 46],
            offset,
            check,
        )?);
        at = next;
    }
    if at != directory.len() {
        return Err(invalid("Extra ZIP directory data."));
    }
    ranges.sort_unstable();
    if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(invalid("Overlapping ZIP local records."));
    }
    // Relocate only the in-memory central directory to offset zero. Local
    // offsets still address the original file. The parser receives the exact
    // validated bytes and a comment-free ZIP32 footer, never the source tail.
    let directory_end = directory.len() as u64;
    let mut footer: [u8; 22] = tail[end..end + 22].try_into().unwrap();
    footer[16..22].fill(0);
    directory.extend_from_slice(&footer);
    Ok(Preflight {
        members: names,
        index: IndexReader {
            bytes: io::Cursor::new(directory),
            directory_end,
            footer_selected: false,
            footer_read: false,
            reading_directory: false,
        },
    })
}

/// A single-directory parser view for locked ZIP 6.0.0. Footer discovery sees
/// zeroes before the canonical footer, so it cannot discover another record.
/// After selecting that footer, Known(0) probes the validated directory. The
/// four-byte probe may rewind; subsequent directory parsing is sequential and
/// excludes the footer. A parser error therefore cannot retry another EOCD,
/// even if arbitrary CRC/extra/comment bytes contain EOCD signatures.
struct IndexReader {
    bytes: io::Cursor<Vec<u8>>,
    directory_end: u64,
    footer_selected: bool,
    footer_read: bool,
    reading_directory: bool,
}
impl Read for IndexReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let at = self.bytes.position();
        if self.reading_directory {
            let remaining = self.directory_end.saturating_sub(at) as usize;
            let length = output.len().min(remaining);
            return self.bytes.read(&mut output[..length]);
        }
        let length = self.bytes.read(output)?;
        let hidden = self.directory_end.saturating_sub(at).min(length as u64) as usize;
        output[..hidden].fill(0);
        if self.footer_selected && self.bytes.position() == self.bytes.get_ref().len() as u64 {
            self.footer_read = true;
        }
        Ok(length)
    }
}
impl Seek for IndexReader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let current = self.bytes.position();
        let position = match from {
            SeekFrom::Start(position) => Some(position),
            SeekFrom::Current(delta) => current.checked_add_signed(delta),
            SeekFrom::End(delta) => (self.bytes.get_ref().len() as u64).checked_add_signed(delta),
        }
        .ok_or_else(|| io::Error::other("Invalid ZIP index seek."))?;
        if self.reading_directory {
            if position != current && !(current <= 4 && position == 0) {
                return Err(io::Error::other("ZIP directory fallback is forbidden."));
            }
        } else if self.footer_read && position == 0 {
            self.reading_directory = true;
        } else {
            self.footer_selected = position == self.directory_end;
        }
        self.bytes.set_position(position);
        Ok(position)
    }
}

/// Original file access is enabled only after the bounded index succeeds.
struct ArchiveReader<'a> {
    input: File,
    index: IndexReader,
    initialized: &'a Cell<bool>,
}
impl Read for ArchiveReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.initialized.get() {
            self.input.read(output)
        } else {
            self.index.read(output)
        }
    }
}
impl Seek for ArchiveReader<'_> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        if self.initialized.get() {
            self.input.seek(from)
        } else {
            self.index.seek(from)
        }
    }
}

fn json_member<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    name: &str,
    limits: Limits,
    check: &mut Check<'_>,
) -> Result<String, Error> {
    let mut member = zip.by_name(name)?;
    bounded(member.size(), limits.json_bytes)?;
    let mut bytes = Vec::new();
    copy(
        &mut member,
        &mut bytes,
        limits.json_bytes,
        Stage::Read,
        check,
    )?;
    String::from_utf8(bytes).map_err(|_| invalid("Invalid JSON encoding."))
}

pub fn extract(
    source: &Path,
    managed: &Path,
    limits: Limits,
    check: &mut Check<'_>,
) -> Result<Extracted, Error> {
    let mut input = File::open(source)?;
    let Preflight { members, index } = preflight(&mut input, limits, check)?;
    let initialized = Cell::new(false);
    let mut zip = ZipArchive::with_config(
        zip::read::Config {
            archive_offset: zip::read::ArchiveOffset::Known(0),
        },
        ArchiveReader {
            input,
            index,
            initialized: &initialized,
        },
    )?;
    initialized.set(true);
    if zip.len() != members.len() {
        return Err(invalid("Ambiguous ZIP directory."));
    }
    let manifest: Manifest =
        serde_json::from_str(&json_member(&mut zip, MANIFEST, limits, check)?)?;
    if manifest.schema != SCHEMA || manifest.project != PROJECT {
        return Err(invalid("Unsupported project archive schema."));
    }
    let json = json_member(&mut zip, PROJECT, limits, check)?;
    let project = file::from_json(&json).map_err(|error| invalid(error.to_string()))?;
    let session = file::session_from_json(&json);
    if manifest.audio.len() != project.samples.len() {
        return Err(invalid("Archive audio manifest does not match project."));
    }
    let mut expected = HashSet::from([MANIFEST.to_owned(), PROJECT.to_owned()]);
    let mut samples = HashSet::new();
    let mut audio_paths = HashSet::new();
    let mut extracted_bytes: u64 = members.values().sum();
    for audio in &manifest.audio {
        member_name(&audio.member)?;
        member_name(&audio.path)?;
        extracted_bytes = extracted_bytes
            .checked_add(audio.bytes)
            .ok_or_else(|| invalid("Archive size overflow."))?;
        bounded(extracted_bytes, limits.expanded_bytes)?;
        if !audio.member.starts_with("audio/")
            || audio.member.matches('/').count() != 1
            || !audio.path.starts_with("samples/")
            || audio.path.matches('/').count() != 1
            || !audio_paths.insert(audio.path.to_ascii_lowercase())
            || !samples.insert(audio.sample)
            || members.get(&audio.member) != Some(&audio.bytes)
            || project
                .samples
                .iter()
                .find(|sample| sample.id == audio.sample)
                .is_none_or(|sample| sample.path != SamplePath::Project(audio.path.clone()))
        {
            return Err(invalid("Archive audio is missing or mismatched."));
        }
        expected.insert(audio.member.clone());
    }
    if expected.len() != members.len() || members.keys().any(|name| !expected.contains(name)) {
        return Err(invalid("Archive contains unlisted members."));
    }
    // Validate all metadata before creating any extraction destination.
    fs::create_dir_all(managed)?;
    let root = tempfile::Builder::new()
        .prefix("project-")
        .tempdir_in(managed)?;
    let owner = File::options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(root.path().join(OWNER_LOCK))?;
    owner.lock()?;
    file::write_atomic(
        &root.path().join(OWNERSHIP),
        &serde_json::to_vec(&ExtractionRecord {
            version: 1,
            ..Default::default()
        })?,
    )
    .map_err(|error| {
        invalid(
            windfall_core::notice!("archive.ownershipNotRecorded", error = error.to_string())
                .to_string(),
        )
    })?;
    fs::create_dir(root.path().join("audio"))?;
    fs::create_dir(root.path().join("samples"))?;
    for name in expected {
        let mut entry = zip.by_name(&name)?;
        if entry.name() != name || entry.is_symlink() || entry.is_dir() {
            return Err(invalid("Ambiguous ZIP member."));
        }
        let mut target = File::options()
            .write(true)
            .create_new(true)
            .open(root.path().join(&name))?;
        let (bytes, _) = copy(
            &mut entry,
            &mut target,
            *members
                .get(&name)
                .ok_or_else(|| invalid("Missing ZIP member."))?,
            Stage::Extract,
            check,
        )?;
        if bytes != members[&name] {
            return Err(invalid("Truncated archive member."));
        }
        target.sync_all()?;
    }
    for audio in &manifest.audio {
        let mut from = File::open(root.path().join(&audio.member))?;
        let mut to = File::options()
            .write(true)
            .create_new(true)
            .open(root.path().join(&audio.path))?;
        copy(&mut from, &mut to, audio.bytes, Stage::Extract, check)?;
        to.sync_all()?;
    }
    check(Stage::Ready)?;
    Ok(Extracted {
        owner,
        root,
        project,
        session,
    })
}
