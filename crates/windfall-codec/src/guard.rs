//! Keeps metadata away from Symphonia unless it is small.
//!
//! Windfall shows no tags, pictures, markers or cue sheets, but Symphonia's
//! readers load all of them, into memory sized by lengths the file states.
//! The limits in its `MetadataOptions` are not read anywhere in 0.6, so a
//! file a few bytes long can ask for megabytes, and a failed allocation
//! aborts the process.
//!
//! [`open`] is Symphonia's own format detection with two changes. An ID3v2
//! tag in front of a file is skipped unread unless it is small. And each
//! container reader gets its bytes through a [`Guard`], which follows the
//! container's framing and rewrites the header of any metadata over the
//! budget so that the reader steps over it. Nothing is cut out, so every
//! offset in the file stays where it was, and a file with little metadata
//! passes through byte for byte.
//!
//! The guard is also where a file is turned down for stating more channels
//! or a higher sample rate than any real file has. Symphonia sizes buffers by
//! the channel count before it has read any audio, so the header that states
//! it must not reach the reader: see [`check_claim`].

use std::cell::Cell;
use std::collections::VecDeque;
use std::io::{self, Read, Seek, SeekFrom};
use std::ops::Range;
use std::sync::LazyLock;

use symphonia::core::errors::{Error, Result, limit_error};
use symphonia::core::formats::probe::{
    Hint, Probe, ProbeFormatData, ProbeMetadataData, ProbeableFormat, ProbeableMetadata, Score,
    Scoreable,
};
use symphonia::core::formats::{
    Attachment, FormatInfo, FormatOptions, FormatReader, MediaInfo, SeekMode, SeekTo, SeekedTo,
    Track, TrackType,
};
use symphonia::core::io::{MediaSource, MediaSourceStream, ReadBytes, ScopedStream, SeekBuffered};
use symphonia::core::meta::{
    ChapterGroup, Metadata, MetadataBuffer, MetadataBuilder, MetadataInfo, MetadataOptions,
    MetadataReader,
};
use symphonia::core::packet::Packet;
use symphonia::default::formats::{AiffReader, FlacReader, MpaReader, OggReader, WavReader};
use symphonia::default::meta::{Id3v1Reader, Id3v2Reader};

use crate::error::CodecError;

/// Most channels a file may have: 64, as many as seventh-order ambisonics,
/// the widest layout in use. Symphonia's PCM decoder sets aside 1152 frames
/// for every channel the header states before it has read any audio, so a
/// file of a hundred bytes that states thousands of channels would take tens
/// of megabytes.
pub(crate) const MAX_CHANNELS: usize = 64;

/// Highest sample rate a file may have: 768 kHz, sixteen times 48 kHz and the
/// fastest that converters run at. A header that states more is damaged or
/// made up.
pub(crate) const MAX_SAMPLE_RATE: u32 = 768_000;

/// Most metadata one file may hand to Symphonia, in bytes as stored: 16 KiB
/// for its tags, text chunks, comment blocks and pictures together. Titles,
/// artists and comments come to a few hundred bytes. What makes metadata
/// large is cover art, which starts well above this and which Windfall does
/// not show. Symphonia turns every tag into several heap objects, so the
/// memory taken is a multiple of the bytes let through; with this budget it
/// stays under 1 MiB however the tags are packed.
///
/// Ogg is the exception. Its pages cannot be rewritten without redoing their
/// checksums, so there the limit is on whole pages: see [`PageBlanker`].
const METADATA_BUDGET: u64 = 16 * 1024;

/// What each piece of metadata costs on top of its length. One with no
/// content still becomes an entry in a list, so empty ones must run out too.
const ITEM_COST: u64 = 64;

/// How many ID3v2 tags a file may start with. A real one has one, at times
/// two. The probe keeps an entry for each, even for those skipped unread.
const LEADING_TAGS_MAX: u32 = 64;

/// How many bytes are looked at where a header starts: enough for the 8-byte
/// chunk header and the part of a `fmt `, `COMM` or `ID3 ` chunk that says
/// how far its reader will go.
const HEADER_VIEW: usize = 32;

/// The chunk ID written over a hidden RIFF or AIFF chunk. Symphonia steps
/// over chunks it does not know.
const HIDDEN_CHUNK: &[u8; 4] = b"JUNK";

const FLAC_STREAM_INFO: u8 = 0;
const FLAC_PADDING: u8 = 1;
const FLAC_LAST_BLOCK: u8 = 0x80;

const ID3V2_HEADER_LEN: usize = 10;

const OGG_HEADER_LEN: usize = 27;
const OGG_CONTINUED_PACKET: u8 = 0x01;
const OGG_FIRST_PAGE: u8 = 0x02;
/// The longest Ogg page: a header, 255 segment lengths and 255 full segments.
const OGG_PAGE_MAX: usize = OGG_HEADER_LEN + 255 + 255 * 255;

/// How the comment header packets of Vorbis and Opus begin.
const OGG_COMMENT_PACKETS: [&[u8]; 2] = [b"\x03vorbis", b"OpusTags"];

/// How far into an Ogg stream a comment header may start. The real one is on
/// the second page, 58 bytes in; in a file that also carries video, the
/// headers of the other streams can come first.
const OGG_COMMENT_ZONE: u64 = 64 * 1024;

/// How much of an Ogg file is read at a time to look for pages.
const PAGE_SEARCH_BLOCK: usize = 16 * 1024;

/// Turns down a channel count over [`MAX_CHANNELS`] or a sample rate over
/// [`MAX_SAMPLE_RATE`]. Zero passes: it is what a caller gives for a value
/// that is not stated, and a header that does state it is turned down by its
/// reader or once the file is open.
pub(crate) fn check_claim(
    channels: usize,
    sample_rate: u32,
) -> std::result::Result<(), CodecError> {
    let reason = if channels > MAX_CHANNELS {
        windfall_core::notice!("codec.channelCount", channels = channels).to_string()
    } else if sample_rate > MAX_SAMPLE_RATE {
        windfall_core::notice!("codec.sampleRate", rate = sample_rate).to_string()
    } else {
        return Ok(());
    };
    Err(CodecError::UnsupportedFormat(reason))
}

/// What is left of one file's metadata budget.
#[derive(Clone, Copy)]
struct Budget(u64);

impl Budget {
    /// Takes one piece of metadata of `length` bytes out of the budget, if
    /// that much is left. Returns whether it fit.
    fn admit(&mut self, length: u64) -> bool {
        let left = self.0.checked_sub(length.saturating_add(ITEM_COST));
        if let Some(left) = left {
            self.0 = left;
        }
        left.is_some()
    }
}

/// What the file being opened may still hand to Symphonia.
#[derive(Clone, Copy)]
struct Allowance {
    budget: Budget,
    leading_tags: u32,
}

thread_local! {
    /// The allowance of the file this thread is opening. Symphonia's probe
    /// hands the readers it starts nothing they could share, so this is how
    /// the tags in front of a file and the metadata inside it come out of
    /// one budget.
    static ALLOWANCE: Cell<Allowance> = const {
        Cell::new(Allowance {
            budget: Budget(0),
            leading_tags: 0,
        })
    };
}

/// Finds the container in `stream` and opens Symphonia's reader for it, with
/// the file's metadata behind the guard. Use it in place of the probe from
/// `symphonia::default`.
pub(crate) fn open<'s>(
    stream: MediaSourceStream<'s>,
    hint: &Hint,
) -> Result<Box<dyn FormatReader + 's>> {
    // The formats and their order are those of Symphonia's default probe. A
    // format switched on in `Cargo.toml` is not read until it is listed here.
    static PROBE: LazyLock<Probe> = LazyLock::new(|| {
        let mut probe = Probe::new();
        probe.register_format::<Guarded<FlacReader<'_>>>();
        // The MPEG audio reader reads no metadata of its own.
        probe.register_format::<MpaReader<'_>>();
        probe.register_format::<Guarded<AiffReader<'_>>>();
        probe.register_format::<Guarded<WavReader<'_>>>();
        probe.register_format::<Guarded<OggReader<'_>>>();
        // An ID3v1 tag is 128 bytes, always.
        probe.register_metadata::<Id3v1Reader<'_>>();
        probe.register_metadata::<SkippedTag<'_>>();
        probe
    });

    ALLOWANCE.set(Allowance {
        budget: Budget(METADATA_BUDGET),
        leading_tags: LEADING_TAGS_MAX,
    });
    PROBE.probe(
        hint,
        stream,
        FormatOptions::default(),
        MetadataOptions::default(),
    )
}

/// The length an ID3v2 header gives for the rest of its tag: four bytes of
/// seven bits each.
fn id3v2_body_len(header: &[u8; ID3V2_HEADER_LEN]) -> u32 {
    header[6..]
        .iter()
        .fold(0, |length, &byte| (length << 7) | u32::from(byte & 0x7F))
}

/// Stands in for Symphonia's ID3v2 reader on a tag that does not fit the
/// budget, and steps over the tag without reading it.
struct SkippedTag<'s> {
    stream: MediaSourceStream<'s>,
    body_len: u64,
}

impl Scoreable for SkippedTag<'_> {
    fn score(source: ScopedStream<&mut MediaSourceStream<'_>>) -> Result<Score> {
        Id3v2Reader::score(source)
    }
}

impl<'s> ProbeableMetadata<'s> for SkippedTag<'_> {
    fn try_probe_new(
        mut stream: MediaSourceStream<'s>,
        options: MetadataOptions,
    ) -> Result<Box<dyn MetadataReader + 's>> {
        let mut header = [0; ID3V2_HEADER_LEN];
        ReadBytes::read_buf_exact(&mut stream, &mut header)?;
        let body_len = u64::from(id3v2_body_len(&header));

        let mut allowance = ALLOWANCE.get();
        let Some(tags_left) = allowance.leading_tags.checked_sub(1) else {
            return limit_error("it starts with too many tags");
        };
        allowance.leading_tags = tags_left;
        let fits = allowance.budget.admit(ID3V2_HEADER_LEN as u64 + body_len);
        ALLOWANCE.set(allowance);

        if fits {
            stream.seek_buffered_rev(ID3V2_HEADER_LEN);
            return Id3v2Reader::try_probe_new(stream, options);
        }
        Ok(Box::new(SkippedTag { stream, body_len }))
    }

    fn probe_data() -> &'static [ProbeMetadataData] {
        Id3v2Reader::probe_data()
    }
}

impl MetadataReader for SkippedTag<'_> {
    fn metadata_info(&self) -> &MetadataInfo {
        &Id3v2Reader::probe_data()[0].info
    }

    fn read_all(&mut self) -> Result<MetadataBuffer> {
        // Seeking past the end of a file succeeds, so a tag that is cut off
        // has to be noticed here.
        let ends_at = self.stream.pos().saturating_add(self.body_len);
        if self.stream.byte_len().is_some_and(|len| ends_at > len) {
            return Err(Error::IoError(io::ErrorKind::UnexpectedEof.into()));
        }
        self.stream.ignore_bytes(self.body_len)?;
        Ok(MetadataBuffer {
            revision: MetadataBuilder::new(*self.metadata_info()).build(),
            side_data: Vec::new(),
        })
    }

    fn into_inner<'s>(self: Box<Self>) -> MediaSourceStream<'s>
    where
        Self: 's,
    {
        self.stream
    }
}

/// One of Symphonia's container readers, reading through a [`Guard`].
struct Guarded<R>(R);

impl<R: FormatReader> FormatReader for Guarded<R> {
    fn format_info(&self) -> &FormatInfo {
        self.0.format_info()
    }

    fn media_info(&self) -> &MediaInfo {
        self.0.media_info()
    }

    fn attachments(&self) -> &[Attachment] {
        self.0.attachments()
    }

    fn chapters(&self) -> Option<&ChapterGroup> {
        self.0.chapters()
    }

    fn metadata(&mut self) -> Metadata<'_> {
        self.0.metadata()
    }

    fn seek(&mut self, mode: SeekMode, to: SeekTo) -> Result<SeekedTo> {
        self.0.seek(mode, to)
    }

    fn tracks(&self) -> &[Track] {
        self.0.tracks()
    }

    fn first_track(&self, track_type: TrackType) -> Option<&Track> {
        self.0.first_track(track_type)
    }

    fn first_track_known_codec(&self, track_type: TrackType) -> Option<&Track> {
        self.0.first_track_known_codec(track_type)
    }

    fn default_track(&self, track_type: TrackType) -> Option<&Track> {
        self.0.default_track(track_type)
    }

    fn next_packet(&mut self) -> Result<Option<Packet>> {
        self.0.next_packet()
    }

    fn into_inner<'s>(self: Box<Self>) -> MediaSourceStream<'s>
    where
        Self: 's,
    {
        Box::new(self.0).into_inner()
    }
}

/// Lets the probe find `$reader` exactly where it would find it unguarded,
/// and opens it on a stream that starts at the container and hides the
/// metadata that is over the budget.
macro_rules! guard_format {
    ($reader:ident, $filter:expr) => {
        impl Scoreable for Guarded<$reader<'_>> {
            fn score(source: ScopedStream<&mut MediaSourceStream<'_>>) -> Result<Score> {
                $reader::score(source)
            }
        }

        impl<'s> ProbeableFormat<'s> for Guarded<$reader<'_>> {
            fn try_probe_new(
                stream: MediaSourceStream<'s>,
                options: FormatOptions,
            ) -> Result<Box<dyn FormatReader + 's>> {
                let guard = Guard::new(stream, $filter);
                let stream = MediaSourceStream::new(Box::new(guard), Default::default());
                Ok(Box::new(Guarded($reader::try_new(stream, options)?)))
            }

            fn probe_data() -> &'static [ProbeFormatData] {
                $reader::probe_data()
            }
        }
    };
}

guard_format!(
    WavReader,
    Filter::Headers(HeaderWalk::new(Layout::Wave, ALLOWANCE.get().budget))
);
guard_format!(
    AiffReader,
    Filter::Headers(HeaderWalk::new(
        Layout::Aiff { compressed: false },
        ALLOWANCE.get().budget
    ))
);
guard_format!(
    FlacReader,
    Filter::Headers(HeaderWalk::new(Layout::Flac, ALLOWANCE.get().budget))
);
guard_format!(OggReader, Filter::Pages(PageBlanker::default()));

/// The bytes of one container as its Symphonia reader gets to see them: the
/// same bytes at the same offsets, counted from where the container starts,
/// except that metadata over the budget is made to look like something the
/// reader skips.
///
/// Reading stops with an error at a header that [`check_claim`] turns down.
/// The I/O error holds the [`CodecError`] to give the user.
struct Guard<'s> {
    inner: MediaSourceStream<'s>,
    /// Where the container starts in `inner`.
    base: u64,
    position: u64,
    filter: Filter,
}

enum Filter {
    Headers(HeaderWalk),
    Pages(PageBlanker),
}

impl<'s> Guard<'s> {
    /// Guards the container that starts where `inner` stands.
    fn new(inner: MediaSourceStream<'s>, filter: Filter) -> Self {
        Self {
            base: inner.pos(),
            inner,
            position: 0,
            filter,
        }
    }
}

impl Read for Guard<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let count = match &mut self.filter {
            Filter::Headers(walk) => walk.read(&mut self.inner, self.position, out)?,
            Filter::Pages(pages) => pages.read(&mut self.inner, out)?,
        };
        self.position += count as u64;
        Ok(count)
    }
}

impl Seek for Guard<'_> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::Current(delta) => self.position.checked_add_signed(delta),
            SeekFrom::End(delta) => self
                .byte_len()
                .and_then(|len| len.checked_add_signed(delta)),
        }
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;

        match &mut self.filter {
            Filter::Headers(walk) => {
                walk.seek(&mut self.inner, self.base, &mut self.position, target)?;
            }
            Filter::Pages(pages) => {
                pages.seek(&mut self.inner, self.base, target)?;
                self.position = target;
            }
        }
        Ok(target)
    }
}

impl MediaSource for Guard<'_> {
    fn is_seekable(&self) -> bool {
        self.inner.is_seekable()
    }

    fn byte_len(&self) -> Option<u64> {
        self.inner
            .byte_len()
            .map(|len| len.saturating_sub(self.base))
    }
}

/// Reads until `buffer` holds `length` bytes or the source ends.
fn fill(source: &mut impl Read, buffer: &mut Vec<u8>, length: usize) -> io::Result<()> {
    let mut filled = buffer.len();
    buffer.resize(length.max(filled), 0);
    let outcome = loop {
        if filled == buffer.len() {
            break Ok(());
        }
        match source.read(&mut buffer[filled..]) {
            Ok(0) => break Ok(()),
            Ok(count) => filled += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => break Err(error),
        }
    };
    buffer.truncate(filled);
    outcome
}

/// Follows the chunk or block headers of a WAV, AIFF or FLAC file in the
/// order Symphonia's reader meets them, and hides the metadata that does not
/// fit the budget before the reader gets to see its header. The header that
/// states the channel count and sample rate is checked on the way.
struct HeaderWalk {
    layout: Layout,
    /// The budget the walk starts out with.
    budget: Budget,
    budget_left: Budget,
    /// Offset of the next header not looked at yet. Everything before it is
    /// settled and may be handed out.
    next_header: u64,
    /// Bytes read from the file beyond the reader's position, to look at a
    /// header. They already carry what was rewritten.
    ahead: Vec<u8>,
}

impl HeaderWalk {
    fn new(layout: Layout, budget: Budget) -> Self {
        Self {
            layout,
            budget,
            budget_left: budget,
            next_header: 0,
            ahead: Vec::new(),
        }
    }

    fn read(
        &mut self,
        inner: &mut MediaSourceStream<'_>,
        position: u64,
        out: &mut [u8],
    ) -> io::Result<usize> {
        // A header that starts among the bytes about to be handed out is
        // looked at first.
        while self.next_header - position < self.ahead.len().max(1) as u64 {
            self.look(inner, position)?;
        }
        let settled = usize::try_from(self.next_header - position).unwrap_or(usize::MAX);
        let count = out.len().min(settled);
        if self.ahead.is_empty() {
            return inner.read(&mut out[..count]);
        }
        let count = count.min(self.ahead.len());
        out[..count].copy_from_slice(&self.ahead[..count]);
        self.ahead.drain(..count);
        Ok(count)
    }

    /// Looks at the header at `next_header`, which starts in or right behind
    /// the bytes read ahead of `position`, and finds the one after it.
    fn look(&mut self, inner: &mut MediaSourceStream<'_>, position: u64) -> io::Result<()> {
        let start = (self.next_header - position) as usize;
        fill(inner, &mut self.ahead, start + HEADER_VIEW)?;
        let header = &mut self.ahead[start..];
        if let Some((channels, sample_rate)) = self.layout.claim(self.next_header, header) {
            // The walk does not move past a header it refuses, so every
            // later read is refused as well.
            check_claim(channels, sample_rate).map_err(io::Error::other)?;
        }
        // Past the last header that matters, nothing is looked at any more.
        self.next_header = self
            .layout
            .inspect(self.next_header, header, &mut self.budget_left)
            .unwrap_or(u64::MAX);
        Ok(())
    }

    /// Moves the reading position to `to`, at or after `position`.
    fn jump(
        &mut self,
        inner: &mut MediaSourceStream<'_>,
        base: u64,
        position: &mut u64,
        to: u64,
    ) -> io::Result<()> {
        match usize::try_from(to - *position) {
            Ok(within) if within <= self.ahead.len() => {
                self.ahead.drain(..within);
            }
            _ => {
                inner.seek(SeekFrom::Start(base.saturating_add(to)))?;
                self.ahead.clear();
            }
        }
        *position = to;
        Ok(())
    }

    fn seek(
        &mut self,
        inner: &mut MediaSourceStream<'_>,
        base: u64,
        position: &mut u64,
        target: u64,
    ) -> io::Result<()> {
        // What is hidden at `target` depends on every header before it, so
        // going back means following them again from the top.
        if target < *position {
            inner.seek(SeekFrom::Start(base))?;
            *self = Self::new(self.layout.restarted(), self.budget);
            *position = 0;
        }
        while self.next_header <= target {
            self.jump(inner, base, position, self.next_header)?;
            self.look(inner, *position)?;
        }
        self.jump(inner, base, position, target)
    }
}

/// How the headers of each container follow one another.
#[derive(Clone, Copy)]
enum Layout {
    Wave,
    /// `compressed` is set once the file has said it is AIFF-C.
    Aiff {
        compressed: bool,
    },
    Flac,
}

impl Layout {
    /// The layout as it is before the first header.
    fn restarted(self) -> Self {
        match self {
            Self::Aiff { .. } => Self::Aiff { compressed: false },
            other => other,
        }
    }

    /// Looks at the header at `offset`, hides what it announces if that is
    /// metadata over the budget, and returns the offset of the header that
    /// Symphonia reads next. `None` means no later header matters, or the
    /// file ends before this one is complete.
    fn inspect(&mut self, offset: u64, header: &mut [u8], budget: &mut Budget) -> Option<u64> {
        let to_next = match self {
            // The file opens with `RIFF` or `FORM`, a length and a form type.
            Self::Wave if offset == 0 => (header.len() >= 12).then_some(12),
            Self::Wave => wave_chunk(header, budget),
            Self::Aiff { compressed } if offset == 0 => {
                *compressed = header.get(8..12)? == b"AIFC";
                Some(12)
            }
            Self::Aiff { compressed } => aiff_chunk(header, *compressed, budget),
            Self::Flac if offset == 0 => (header.len() >= 4).then_some(4),
            Self::Flac => flac_block(header, budget),
        };
        Some(offset + to_next?)
    }

    /// The channel count and sample rate stated by the header at `offset`,
    /// if it is the one that holds them and the file goes on long enough.
    fn claim(self, offset: u64, header: &[u8]) -> Option<(usize, u32)> {
        // The file's opening bytes state neither.
        if offset == 0 {
            return None;
        }
        match self {
            Self::Wave => {
                // The chunk's length and the format tag come first.
                let stated = header.strip_prefix(b"fmt ")?.get(6..)?;
                let (channels, rest) = stated.split_first_chunk()?;
                let sample_rate = u32::from_le_bytes(*rest.first_chunk()?);
                Some((usize::from(u16::from_le_bytes(*channels)), sample_rate))
            }
            Self::Aiff { .. } => {
                let stated = header.strip_prefix(b"COMM")?.get(4..)?;
                let (channels, rest) = stated.split_first_chunk()?;
                // The frame count and the sample size lie in between.
                let sample_rate = aiff_sample_rate(rest.get(6..)?.first_chunk()?);
                Some((usize::from(u16::from_be_bytes(*channels)), sample_rate))
            }
            Self::Flac => {
                let (kind, rest) = header.split_first()?;
                if kind & !FLAC_LAST_BLOCK != FLAC_STREAM_INFO {
                    return None;
                }
                // Behind the block's length and ten bytes of block and frame
                // sizes: 20 bits of sample rate, then 3 of channels less one.
                let packed: &[u8; 3] = rest.get(13..)?.first_chunk()?;
                let sample_rate = u32::from_be_bytes([0, packed[0], packed[1], packed[2]]) >> 4;
                Some((usize::from((packed[2] >> 1) & 0x07) + 1, sample_rate))
            }
        }
    }
}

/// The sample rate of an AIFF common chunk, which is an 80-bit float, in
/// whole hertz as Symphonia takes it. A rate too high for the type comes out
/// as the highest there is. A negative one and one that is no number come
/// out as zero; the reader refuses those.
fn aiff_sample_rate(bytes: &[u8; 10]) -> u32 {
    let [high, low, mantissa @ ..] = *bytes;
    let head = u16::from_be_bytes([high, low]);
    // The sign bit, or the exponent that marks infinity and what is no number.
    if head >= 0x7FFF {
        return 0;
    }
    // The value is the mantissa times two to the power of `exponent - 63`.
    let mantissa = u64::from_be_bytes(mantissa);
    let exponent = i32::from(head) - 16_383;
    let whole = match exponent {
        ..0 => 0,
        0..=63 => u128::from(mantissa >> (63 - exponent)),
        _ => u128::from(mantissa) << (exponent - 63).min(64),
    };
    u32::try_from(whole).unwrap_or(u32::MAX)
}

/// Splits what was read at a chunk header into the chunk's ID, the bytes of
/// its length and the start of its body.
fn split_chunk(header: &mut [u8]) -> Option<(&mut [u8; 4], [u8; 4], &[u8])> {
    let (id, rest) = header.split_first_chunk_mut()?;
    let (length, body) = rest.split_first_chunk()?;
    Some((id, *length, body))
}

/// Looks at a WAV chunk header and returns how far behind it Symphonia reads
/// the next one.
fn wave_chunk(header: &mut [u8], budget: &mut Budget) -> Option<u64> {
    let (id, length, body) = split_chunk(header)?;
    let length = u32::from_le_bytes(length);
    let read = match &*id {
        // The audio follows, and with it the end of the headers.
        b"data" => return None,
        b"fmt " => wave_format_read(body, length)?,
        b"LIST" => {
            // Symphonia skips two padding bytes behind a list of odd length
            // and loses its place, so such a list is hidden whatever its
            // size.
            if length % 2 == 1 || !budget.admit(u64::from(length)) {
                *id = *HIDDEN_CHUNK;
            }
            u64::from(length)
        }
        _ => u64::from(length),
    };
    Some(8 + read + u64::from(length % 2))
}

/// How many bytes Symphonia reads of a `fmt ` chunk with this `body` and
/// stated `length`. Some formats carry extra bytes that are read by their own
/// count, and of the extensible format exactly 40 bytes are read; either can
/// end short of the chunk or beyond it. `None` if the file ends too soon to
/// tell.
fn wave_format_read(body: &[u8], length: u32) -> Option<u64> {
    const ADPCM: u16 = 0x0002;
    const A_LAW: u16 = 0x0006;
    const MU_LAW: u16 = 0x0007;
    const IMA_ADPCM: u16 = 0x0011;
    const EXTENSIBLE: u16 = 0xFFFE;

    let format = u16::from_le_bytes(*body.first_chunk()?);
    Some(match format {
        ADPCM | A_LAW | MU_LAW | IMA_ADPCM => {
            let extra = u16::from_le_bytes(*body.get(16..)?.first_chunk()?);
            18 + u64::from(extra)
        }
        EXTENSIBLE => 40,
        _ => u64::from(length),
    })
}

/// Looks at an AIFF chunk header and returns how far behind it Symphonia
/// reads the next one.
fn aiff_chunk(header: &mut [u8], compressed: bool, budget: &mut Budget) -> Option<u64> {
    let (id, length, body) = split_chunk(header)?;
    let length = u32::from_be_bytes(length);
    let read = match &*id {
        // The common chunk is read by its own layout, not by its length: 18
        // bytes, and in AIFF-C the codec's ID and its name, which is a length
        // byte and text padded to an odd count.
        b"COMM" if compressed => 23 + u64::from(*body.get(22)? | 1),
        b"COMM" => 18,
        _ => u64::from(length),
    };
    let fits = match &*id {
        // The reader sizes its tables of markers and comments by a count
        // inside the chunk and reads them without regard to the chunk's
        // length, so neither is ever let through.
        b"MARK" | b"COMT" => false,
        b"APPL" | b"NAME" | b"AUTH" | b"(c) " | b"ANNO" => budget.admit(u64::from(length)),
        b"ID3 " => id3_chunk_holds_its_tag(body, length) && budget.admit(u64::from(length)),
        _ => true,
    };
    if !fits {
        *id = *HIDDEN_CHUNK;
    }
    Some(8 + read + u64::from(length % 2))
}

/// Whether an AIFF `ID3 ` chunk of `length` bytes holds the whole tag that
/// starts its `body`. Symphonia reads the tag by the tag's own length, so one
/// that claims more would be read on into the chunks behind it.
fn id3_chunk_holds_its_tag(body: &[u8], length: u32) -> bool {
    body.first_chunk().is_some_and(|header| {
        header.starts_with(b"ID3")
            && ID3V2_HEADER_LEN as u64 + u64::from(id3v2_body_len(header)) <= u64::from(length)
    })
}

/// Looks at a FLAC metadata block header and returns how far behind it the
/// next one is. The last block has none.
fn flac_block(header: &mut [u8], budget: &mut Budget) -> Option<u64> {
    let (kind, rest) = header.split_first_mut()?;
    let length: &[u8; 3] = rest.first_chunk()?;
    let length = u32::from_be_bytes([0, length[0], length[1], length[2]]);
    let last = *kind & FLAC_LAST_BLOCK;
    // Application data, seek table, comments, cue sheet and picture. The seek
    // table is only for seeking, which Windfall leaves alone, and can hold
    // close to a million points.
    if matches!(*kind & !FLAC_LAST_BLOCK, 2..=6) && !budget.admit(u64::from(length)) {
        *kind = last | FLAC_PADDING;
    }
    (last == 0).then_some(4 + u64::from(length))
}

/// Blanks the Ogg pages that would make Symphonia load large metadata.
///
/// Symphonia joins a packet that spans pages in one buffer of up to 16 MiB,
/// then copies it, before it knows what the packet is. A comment header with
/// cover art is such a packet. So every page on which no packet ends is
/// zeroed: the reader finds no page there, sees the gap in the page numbers
/// and drops the packet's loose ends, and no packet longer than two pages is
/// ever put together. Vorbis audio and setup packets are far shorter than
/// one page, and the decoder does not need the comment header. The cut is
/// the same for every stream an Ogg file can carry, so a FLAC frame in Ogg
/// that fills a page of 64 KiB by itself is lost as well.
///
/// That alone would let any number of comment headers through, each up to
/// two pages long. So a page on which a comment header starts is zeroed as
/// well, unless it lies within [`OGG_COMMENT_ZONE`] of the stream's start,
/// which leaves room for the one a real file has.
///
/// A page is judged by its own bytes and its offset alone, so the same bytes
/// come out whether the file is read in order or after a seek, and it is
/// looked for at every offset, as Symphonia does after a damaged page.
///
/// The page that opens a Vorbis stream states its channel count and sample
/// rate. If [`check_claim`] turns those down, the bytes before the page are
/// still handed out, and reading fails where the page begins.
#[derive(Default)]
struct PageBlanker {
    /// Bytes of the file from `window_start` on. The first `taken` of them
    /// have been handed out, the first `searched` searched for pages.
    window: Vec<u8>,
    window_start: u64,
    taken: usize,
    searched: usize,
    /// The pages to blank that reach beyond what has been handed out, in
    /// order and apart from one another.
    blanks: VecDeque<Range<u64>>,
    /// Bytes to drop before handing any out, after a seek.
    skip: u64,
    ended: bool,
}

impl PageBlanker {
    fn read(&mut self, inner: &mut MediaSourceStream<'_>, out: &mut [u8]) -> io::Result<usize> {
        loop {
            let ready = self.search()? - self.taken;
            if ready == 0 {
                if self.ended {
                    return Ok(0);
                }
                self.refill(inner)?;
                continue;
            }
            if self.skip > 0 {
                let dropped = ready.min(usize::try_from(self.skip).unwrap_or(usize::MAX));
                self.taken += dropped;
                self.skip -= dropped as u64;
                continue;
            }

            let from = self.window_start + self.taken as u64;
            let until = |offset: u64| usize::try_from(offset - from).unwrap_or(usize::MAX);
            let mut count = ready.min(out.len());
            match self.blanks.front().cloned() {
                Some(blank) if blank.end <= from => {
                    self.blanks.pop_front();
                    continue;
                }
                Some(blank) if blank.start <= from => {
                    count = count.min(until(blank.end));
                    out[..count].fill(0);
                }
                blank => {
                    if let Some(blank) = blank {
                        count = count.min(until(blank.start));
                    }
                    out[..count].copy_from_slice(&self.window[self.taken..self.taken + count]);
                }
            }
            self.taken += count;
            return Ok(count);
        }
    }

    /// Searches the window for pages to blank and returns how many of its
    /// bytes are settled. Fails once a page that is refused is the next thing
    /// to hand out.
    fn search(&mut self) -> io::Result<usize> {
        // A page is judged by all of its bytes, which must be in the window
        // before the first of them is handed out.
        let end = if self.ended {
            self.window.len()
        } else {
            self.window.len().saturating_sub(OGG_PAGE_MAX - 1)
        };
        while self.searched < end {
            // Most bytes cannot start a page, and are passed over quickly.
            let unsearched = &self.window[self.searched..end];
            let Some(skipped) = unsearched.iter().position(|&byte| byte == b'O') else {
                self.searched = end;
                break;
            };
            self.searched += skipped;
            let start = self.window_start + self.searched as u64;
            let page = &self.window[self.searched..];
            if let Some((channels, sample_rate)) = vorbis_claim(page)
                && let Err(refusal) = check_claim(channels, sample_rate)
            {
                // What lies before the page is handed out first. The search
                // never moves past the page, so every later read meets it
                // again.
                if self.searched > self.taken {
                    break;
                }
                return Err(io::Error::other(refusal));
            }
            if let Some(length) = page_to_blank(page, start) {
                match self.blanks.back_mut() {
                    Some(last) if start <= last.end => last.end = last.end.max(start + length),
                    _ => self.blanks.push_back(start..start + length),
                }
            }
            self.searched += 1;
        }
        Ok(self.searched)
    }

    fn refill(&mut self, inner: &mut MediaSourceStream<'_>) -> io::Result<()> {
        self.window.drain(..self.taken);
        self.window_start += self.taken as u64;
        self.searched -= self.taken;
        self.taken = 0;

        let kept = self.window.len();
        fill(inner, &mut self.window, kept + PAGE_SEARCH_BLOCK)?;
        self.ended = self.window.len() == kept;
        Ok(())
    }

    fn seek(
        &mut self,
        inner: &mut MediaSourceStream<'_>,
        base: u64,
        target: u64,
    ) -> io::Result<()> {
        // A page that started up to one page length earlier may reach the
        // target, so reading resumes that far back.
        let start = target.saturating_sub(OGG_PAGE_MAX as u64 - 1);
        inner.seek(SeekFrom::Start(base.saturating_add(start)))?;
        *self = Self {
            window_start: start,
            skip: target - start,
            ..Self::default()
        };
        Ok(())
    }
}

/// The channel count and sample rate in a Vorbis identification header, if
/// `bytes` start with the Ogg page that opens a stream with one.
fn vorbis_claim(bytes: &[u8]) -> Option<(usize, u32)> {
    if !bytes.starts_with(b"OggS") || *bytes.get(5)? & OGG_FIRST_PAGE == 0 {
        return None;
    }
    let segments = usize::from(*bytes.get(OGG_HEADER_LEN - 1)?);
    let packet = bytes.get(OGG_HEADER_LEN + segments..)?;
    // The packet's name and the only version there is, without which
    // Symphonia does not take the stream for Vorbis.
    let stated = packet.strip_prefix(b"\x01vorbis\0\0\0\0")?;
    let (&channels, rest) = stated.split_first()?;
    Some((
        usize::from(channels),
        u32::from_le_bytes(*rest.first_chunk()?),
    ))
}

/// The length of the Ogg page that starts `bytes`, `offset` bytes into the
/// stream, if it is one to blank: a page on which no packet ends, or one
/// beyond the comment zone on which a comment header starts.
fn page_to_blank(bytes: &[u8], offset: u64) -> Option<u64> {
    if !bytes.starts_with(b"OggS") {
        return None;
    }
    let flags = *bytes.get(5)?;
    let segments = usize::from(*bytes.get(OGG_HEADER_LEN - 1)?);
    let table = bytes.get(OGG_HEADER_LEN..OGG_HEADER_LEN + segments)?;
    let body_len: usize = table.iter().map(|&length| usize::from(length)).sum();
    let length = (OGG_HEADER_LEN + segments + body_len) as u64;

    if segments > 0 && table.iter().all(|&length| length == 255) {
        return Some(length);
    }
    if offset < OGG_COMMENT_ZONE {
        return None;
    }
    // A page cut off by the end of the file is none to Symphonia either.
    let body = bytes.get(OGG_HEADER_LEN + segments..)?.get(..body_len)?;
    let mut packet_starts = flags & OGG_CONTINUED_PACKET == 0;
    let mut at = 0;
    for &segment in table {
        let is_comment = |start: &&[u8]| body[at..].starts_with(start);
        if packet_starts && OGG_COMMENT_PACKETS.iter().any(is_comment) {
            return Some(length);
        }
        at += usize::from(segment);
        packet_starts = segment < 255;
    }
    None
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Cursor;
    use std::path::Path;

    use symphonia::core::io::ReadOnlySource;

    use super::*;

    /// As long as the whole budget, which is too much for one piece of
    /// metadata once its own cost is added.
    const TOO_MUCH: usize = METADATA_BUDGET as usize;

    type MakeFilter = fn() -> Filter;

    fn wave() -> Filter {
        Filter::Headers(HeaderWalk::new(Layout::Wave, Budget(METADATA_BUDGET)))
    }

    fn aiff() -> Filter {
        let layout = Layout::Aiff { compressed: false };
        Filter::Headers(HeaderWalk::new(layout, Budget(METADATA_BUDGET)))
    }

    fn flac() -> Filter {
        Filter::Headers(HeaderWalk::new(Layout::Flac, Budget(METADATA_BUDGET)))
    }

    fn ogg() -> Filter {
        Filter::Pages(PageBlanker::default())
    }

    fn guard(bytes: &[u8], filter: Filter, seekable: bool) -> Guard<'_> {
        let source: Box<dyn MediaSource + '_> = if seekable {
            Box::new(Cursor::new(bytes))
        } else {
            Box::new(ReadOnlySource::new(Cursor::new(bytes)))
        };
        Guard::new(MediaSourceStream::new(source, Default::default()), filter)
    }

    /// What the reader behind a guard gets to see of `bytes`, read in order
    /// from a source that can seek and from one that cannot.
    fn seen(bytes: &[u8], filter: MakeFilter) -> Vec<u8> {
        let [seeking, streaming] = [true, false].map(|seekable| {
            let mut seen = Vec::new();
            guard(bytes, filter(), seekable)
                .read_to_end(&mut seen)
                .unwrap();
            seen
        });
        assert!(seeking == streaming, "seeking and streaming differ");
        seeking
    }

    /// `bytes` with each of `texts` written over it at the given offset.
    fn with(bytes: &[u8], texts: &[(usize, &[u8])]) -> Vec<u8> {
        let mut bytes = bytes.to_vec();
        for &(at, text) in texts {
            bytes[at..at + text.len()].copy_from_slice(text);
        }
        bytes
    }

    fn chunk(id: &[u8; 4], length: [u8; 4], body: &[u8]) -> Vec<u8> {
        let mut bytes = id.to_vec();
        bytes.extend(length);
        bytes.extend(body);
        if body.len() % 2 == 1 {
            bytes.push(0);
        }
        bytes
    }

    fn riff_chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        chunk(id, (body.len() as u32).to_le_bytes(), body)
    }

    fn aiff_chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        chunk(id, (body.len() as u32).to_be_bytes(), body)
    }

    /// A WAV file of these chunks behind a 16-byte PCM format chunk.
    fn wave_file(chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = b"RIFF\xFF\xFF\xFF\xFFWAVE".to_vec();
        bytes.extend(riff_chunk(
            b"fmt ",
            &[1, 0, 1, 0, 0x44, 0xAC, 0, 0, 0x88, 0x58, 1, 0, 2, 0, 16, 0],
        ));
        bytes.extend(chunks.concat());
        bytes
    }

    /// An AIFF file of these chunks behind an 18-byte common chunk.
    fn aiff_file(chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = b"FORM\xFF\xFF\xFF\xF0AIFF".to_vec();
        bytes.extend(aiff_chunk(
            b"COMM",
            &[
                0, 1, 0, 0, 0, 4, 0, 16, 0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0,
            ],
        ));
        bytes.extend(chunks.concat());
        bytes
    }

    fn flac_block(kind: u8, body: &[u8]) -> Vec<u8> {
        let mut bytes = vec![kind];
        bytes.extend(&(body.len() as u32).to_be_bytes()[1..]);
        bytes.extend(body);
        bytes
    }

    /// A FLAC file of these blocks behind its stream info block.
    fn flac_file(blocks: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = b"fLaC".to_vec();
        bytes.extend(flac_block(0, &[0; 34]));
        bytes.extend(blocks.concat());
        bytes
    }

    /// An Ogg page with this segment table, whose body starts with `body`
    /// and is filled up with a byte that is not zero.
    fn ogg_page(flags: u8, table: &[u8], body: &[u8]) -> Vec<u8> {
        let mut bytes = b"OggS\x00".to_vec();
        bytes.push(flags);
        bytes.extend([0x11; 20]);
        bytes.push(table.len() as u8);
        bytes.extend(table);
        let mut body = body.to_vec();
        body.resize(table.iter().map(|&length| usize::from(length)).sum(), 0x55);
        bytes.extend(body);
        bytes
    }

    /// `bytes` with zeros over `blanked`.
    fn with_zeros(bytes: &[u8], blanked: Range<usize>) -> Vec<u8> {
        let mut bytes = bytes.to_vec();
        bytes[blanked].fill(0);
        bytes
    }

    #[test]
    fn the_fixtures_pass_through_byte_for_byte() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let mut checked = 0;
        for entry in fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            let filter = match path.extension().and_then(|extension| extension.to_str()) {
                Some("wav") => wave,
                Some("aiff" | "aifc") => aiff,
                Some("flac") => flac,
                Some("ogg") => ogg,
                _ => continue,
            };
            let bytes = fs::read(&path).unwrap();
            assert!(seen(&bytes, filter) == bytes, "{}", path.display());
            checked += 1;
        }
        assert_eq!(checked, 16);
    }

    #[test]
    fn a_wav_list_over_the_budget_gets_another_name() {
        let small = riff_chunk(b"LIST", b"INFOICMT\x04\x00\x00\x00text");
        let large = riff_chunk(b"LIST", &vec![b'A'; TOO_MUCH]);
        let audio = riff_chunk(b"data", &[1, 2, 3, 4]);
        let bytes = wave_file(&[small.clone(), large.clone(), small.clone(), audio]);

        let large_at = 12 + 24 + small.len();
        assert!(seen(&bytes, wave) == with(&bytes, &[(large_at, b"JUNK")]));
    }

    #[test]
    fn a_wav_list_of_odd_length_gets_another_name() {
        let odd = riff_chunk(b"LIST", b"INFOICMT\x03\x00\x00\x00odd");
        let bytes = wave_file(&[odd, riff_chunk(b"data", &[1, 2, 3, 4])]);
        assert!(seen(&bytes, wave) == with(&bytes, &[(12 + 24, b"JUNK")]));
    }

    #[test]
    fn wav_lists_are_let_through_until_the_budget_is_spent() {
        // Each list costs its 1000 bytes and the fixed cost of an item.
        let list = riff_chunk(b"LIST", &[b'A'; 1_000]);
        let let_through = (METADATA_BUDGET / (1_000 + ITEM_COST)) as usize;
        let mut chunks = vec![list.clone(); let_through + 5];
        chunks.push(riff_chunk(b"data", &[1, 2, 3, 4]));
        let bytes = wave_file(&chunks);

        let hidden: Vec<(usize, &[u8])> = (let_through..let_through + 5)
            .map(|index| (12 + 24 + index * list.len(), b"JUNK".as_slice()))
            .collect();
        assert!(seen(&bytes, wave) == with(&bytes, &hidden));
    }

    #[test]
    fn nothing_behind_the_wav_headers_is_touched() {
        // Audio that happens to read as a huge list, and a real one behind
        // the audio, where Symphonia never looks.
        let large = riff_chunk(b"LIST", &vec![b'A'; TOO_MUCH]);
        let audio = riff_chunk(b"data", &large);
        let bytes = wave_file(&[audio, large]);
        assert!(seen(&bytes, wave) == bytes);
    }

    #[test]
    fn aiff_text_over_the_budget_gets_another_name() {
        let small = aiff_chunk(b"NAME", b"A short ramp");
        let large = aiff_chunk(b"ANNO", &vec![b'A'; TOO_MUCH]);
        let markers = aiff_chunk(b"MARK", &[0, 0]);
        let sound = aiff_chunk(b"SSND", &[0; 16]);
        let bytes = aiff_file(&[
            small.clone(),
            large.clone(),
            markers,
            sound.clone(),
            large.clone(),
        ]);

        let large_at = 12 + 26 + small.len();
        let markers_at = large_at + large.len();
        let behind_the_audio = markers_at + 10 + sound.len();
        let hidden: [(usize, &[u8]); 3] = [
            (large_at, b"JUNK"),
            (markers_at, b"JUNK"),
            (behind_the_audio, b"JUNK"),
        ];
        assert!(seen(&bytes, aiff) == with(&bytes, &hidden));
    }

    #[test]
    fn aiff_audio_is_never_touched() {
        let large = aiff_chunk(b"ANNO", &vec![b'A'; TOO_MUCH]);
        let mut sound = vec![0; 8];
        sound.extend(&large);
        let bytes = aiff_file(&[aiff_chunk(b"SSND", &sound)]);
        assert!(seen(&bytes, aiff) == bytes);
    }

    #[test]
    fn an_aiff_tag_chunk_must_hold_its_whole_tag() {
        let tag = |claimed: u8, held: usize| {
            let mut body = b"ID3\x04\x00\x00\x00\x00\x00".to_vec();
            body.push(claimed);
            body.resize(10 + held, 0);
            aiff_chunk(b"ID3 ", &body)
        };
        let sound = aiff_chunk(b"SSND", &[0; 16]);

        let whole = aiff_file(&[tag(20, 20), tag(20, 30), sound.clone()]);
        assert!(seen(&whole, aiff) == whole);

        let spilling = aiff_file(&[tag(21, 20), sound.clone()]);
        assert!(seen(&spilling, aiff) == with(&spilling, &[(12 + 26, b"JUNK")]));

        let no_tag = aiff_file(&[aiff_chunk(b"ID3 ", b"not a tag at all"), sound]);
        assert!(seen(&no_tag, aiff) == with(&no_tag, &[(12 + 26, b"JUNK")]));
    }

    #[test]
    fn a_flac_block_over_the_budget_becomes_padding() {
        const PICTURE: u8 = 6;
        const LAST: u8 = 0x80;
        let small = flac_block(PICTURE, &[0; 100]);
        let large = flac_block(PICTURE, &vec![0; TOO_MUCH]);
        let last = flac_block(PICTURE | LAST, &vec![0; TOO_MUCH]);
        // Frames that happen to read as one more huge picture.
        let audio = flac_block(PICTURE, &vec![0xFF; TOO_MUCH]);
        let bytes = flac_file(&[
            small.clone(),
            large.clone(),
            small.clone(),
            last.clone(),
            audio,
        ]);

        let large_at = 4 + 38 + small.len();
        let last_at = large_at + large.len() + small.len();
        let padding: [(usize, &[u8]); 2] = [
            (large_at, &[FLAC_PADDING]),
            (last_at, &[FLAC_PADDING | LAST]),
        ];
        assert!(seen(&bytes, flac) == with(&bytes, &padding));
    }

    #[test]
    fn every_kind_of_flac_metadata_counts_to_the_budget() {
        // Stream info, padding and kinds Symphonia does not know are not
        // loaded, whatever their size.
        for kind in [0, 1, 7, 126] {
            let bytes = flac_file(&[flac_block(kind, &vec![0; TOO_MUCH]), flac_block(0x81, &[])]);
            assert!(seen(&bytes, flac) == bytes, "kind {kind}");
        }
        for kind in 2..=6 {
            let bytes = flac_file(&[flac_block(kind, &vec![0; TOO_MUCH]), flac_block(0x81, &[])]);
            assert!(
                seen(&bytes, flac) == with(&bytes, &[(4 + 38, &[FLAC_PADDING])]),
                "kind {kind}"
            );
        }
    }

    #[test]
    fn an_ogg_page_on_which_no_packet_ends_is_blanked() {
        let first = ogg_page(0x02, &[30], b"\x01vorbis");
        let ending = ogg_page(0, &[255, 255, 7], b"");
        let middle = ogg_page(0x01, &[255; 255], b"");
        let short_middle = ogg_page(0x01, &[255], b"");
        let empty = ogg_page(0, &[], b"");
        let bytes = [
            first.clone(),
            ending.clone(),
            middle.clone(),
            short_middle.clone(),
            empty,
            ending.clone(),
        ]
        .concat();

        let middle_at = first.len() + ending.len();
        let both_end = middle_at + middle.len() + short_middle.len();
        assert!(seen(&bytes, ogg) == with_zeros(&bytes, middle_at..both_end));
    }

    #[test]
    fn an_ogg_page_is_found_wherever_it_starts() {
        // Behind junk, across the blocks the file is read in, and inside
        // another page.
        for junk in [1, 5_000, PAGE_SEARCH_BLOCK - 3, PAGE_SEARCH_BLOCK + 100] {
            let middle = ogg_page(0x01, &[255; 100], b"");
            let mut bytes = vec![0x55; junk];
            bytes.extend(&middle);
            bytes.extend(ogg_page(0, &[40], b""));
            assert!(
                seen(&bytes, ogg) == with_zeros(&bytes, junk..junk + middle.len()),
                "{junk} bytes of junk"
            );
        }

        let inner = ogg_page(0x01, &[255; 200], b"");
        let mut outer = ogg_page(0, &[255, 200], b"");
        let inner_at = outer.len() - 100;
        outer.truncate(inner_at);
        outer.extend(&inner);
        assert!(seen(&outer, ogg) == with_zeros(&outer, inner_at..outer.len()));
    }

    #[test]
    fn an_ogg_comment_header_far_into_the_file_is_blanked() {
        let comment = ogg_page(0, &[100, 20], b"\x03vorbis");
        let second = ogg_page(0, &[3, 100], b"abcOpusTags");
        let continued = ogg_page(0x01, &[100, 20], b"\x03vorbis");
        let audio = ogg_page(0, &[255, 255, 100], b"");

        // Near the start all of them stay.
        let mut bytes = [comment.clone(), second.clone(), continued.clone()].concat();
        assert!(seen(&bytes, ogg) == bytes);

        while bytes.len() < OGG_COMMENT_ZONE as usize {
            bytes.extend(&audio);
        }
        let far = bytes.len();
        bytes.extend([comment.clone(), second.clone(), continued, audio].concat());
        // The page whose first bytes only carry on an earlier packet stays.
        let blanked = far..far + comment.len() + second.len();
        assert!(seen(&bytes, ogg) == with_zeros(&bytes, blanked));
    }

    #[test]
    fn seeking_shows_the_same_bytes_as_reading_in_order() {
        let large_list = riff_chunk(b"LIST", &vec![b'A'; TOO_MUCH]);
        let wave_bytes = wave_file(&[
            riff_chunk(b"LIST", b"INFOICMT\x04\x00\x00\x00text"),
            large_list.clone(),
            riff_chunk(b"junk", b"odd"),
            large_list,
            riff_chunk(b"data", &[7; 5_000]),
        ]);
        let large_text = aiff_chunk(b"ANNO", &vec![b'A'; TOO_MUCH]);
        let aiff_bytes = aiff_file(&[
            large_text.clone(),
            aiff_chunk(b"MARK", &[0, 1, 0, 0]),
            aiff_chunk(b"SSND", &[7; 5_000]),
            aiff_chunk(b"NAME", b"behind the audio"),
            large_text,
        ]);
        let flac_bytes = flac_file(&[
            flac_block(4, &[0; 300]),
            flac_block(6, &vec![0; TOO_MUCH]),
            flac_block(0x82, &vec![0; TOO_MUCH]),
            vec![7; 5_000],
        ]);
        let mut ogg_bytes = Vec::new();
        for round in 0..6 {
            ogg_bytes.extend(ogg_page(0, &[200, 100], b""));
            ogg_bytes.extend(ogg_page(0x01, &[255; 255], b""));
            ogg_bytes.extend(vec![0x55; round * 1_000]);
            ogg_bytes.extend(ogg_page(0, &[90], b"\x03vorbis"));
        }

        let cases: [(&[u8], MakeFilter); 4] = [
            (&wave_bytes, wave),
            (&aiff_bytes, aiff),
            (&flac_bytes, flac),
            (&ogg_bytes, ogg),
        ];
        for (bytes, filter) in cases {
            let in_order = seen(bytes, filter);
            assert!(in_order != bytes, "something is hidden");
            let length = bytes.len() as u64;

            let mut guard = guard(bytes, filter(), true);
            let mut random = 0x2545_F491_4F6C_DD1D_u64;
            let mut next = |bound: u64| {
                random ^= random << 13;
                random ^= random >> 7;
                random ^= random << 17;
                random % bound
            };
            for round in 0..300 {
                // Mostly around the headers, where things are hidden.
                let reach = if round % 3 == 0 { length + 50 } else { 600 };
                let target = next(reach);
                let position = guard.stream_position().unwrap();
                let landed = match round % 3 {
                    0 => guard.seek(SeekFrom::Start(target)),
                    1 => guard.seek(SeekFrom::Current(target as i64 - position as i64)),
                    _ => guard.seek(SeekFrom::End(target as i64 - length as i64)),
                };
                assert_eq!(landed.unwrap(), target);

                let mut read = vec![0; next(3_000) as usize];
                let mut filled = 0;
                while filled < read.len() {
                    match guard.read(&mut read[filled..]).unwrap() {
                        0 => break,
                        count => filled += count,
                    }
                }
                let start = (target as usize).min(in_order.len());
                let end = (start + read.len()).min(in_order.len());
                assert!(read[..filled] == in_order[start..end], "at {target}");
            }
        }
    }

    #[test]
    fn a_container_behind_other_bytes_is_counted_from_its_start() {
        let wave_bytes = wave_file(&[
            riff_chunk(b"LIST", &vec![b'A'; TOO_MUCH]),
            riff_chunk(b"data", &[7; 500]),
        ]);
        let mut bytes = vec![0x55; 300];
        bytes.extend(&wave_bytes);

        let mut stream = MediaSourceStream::new(Box::new(Cursor::new(&bytes)), Default::default());
        stream.ignore_bytes(300).unwrap();
        let mut guard = Guard::new(stream, wave());
        assert_eq!(guard.byte_len(), Some(wave_bytes.len() as u64));

        let in_order = seen(&wave_bytes, wave);
        let mut read = Vec::new();
        guard.read_to_end(&mut read).unwrap();
        assert!(read == in_order);

        assert_eq!(
            guard.seek(SeekFrom::End(-500)).unwrap(),
            wave_bytes.len() as u64 - 500
        );
        assert_eq!(guard.seek(SeekFrom::Start(36)).unwrap(), 36);
        let mut name = [0; 4];
        guard.read_exact(&mut name).unwrap();
        assert_eq!(&name, HIDDEN_CHUNK);
    }

    /// How many bytes of `bytes` the reader behind a guard gets before it is
    /// refused, and the refusal as the user is shown it.
    fn refused(bytes: &[u8], filter: MakeFilter) -> (usize, String) {
        let [seeking, streaming] = [true, false].map(|seekable| {
            let mut seen = Vec::new();
            let refusal = guard(bytes, filter(), seekable)
                .read_to_end(&mut seen)
                .unwrap_err();
            (
                seen.len(),
                windfall_core::english::english(&refusal.to_string()),
            )
        });
        assert_eq!(seeking, streaming);
        seeking
    }

    #[test]
    fn a_header_that_claims_too_much_fails_the_read() {
        const TOO_MANY: &str = "this is not a supported audio file (it has 65 channels)";
        const TOO_FAST: &str =
            "this is not a supported audio file (it has a sample rate of 768001 Hz)";

        // Each claim sits behind 100 bytes that the reader skips, at most
        // which it gets to see. 768,001 Hz is 0xBB801.
        let wave_claiming = |format: &[u8]| {
            let mut bytes = b"RIFF\xFF\xFF\xFF\xFFWAVE".to_vec();
            bytes.extend(riff_chunk(b"JUNK", &[0; 100]));
            bytes.extend(riff_chunk(b"fmt ", format));
            bytes.extend(riff_chunk(b"data", &[7; 500]));
            bytes
        };
        let aiff_claiming = |common: &[u8]| {
            let mut bytes = b"FORM\xFF\xFF\xFF\xF0AIFF".to_vec();
            bytes.extend(aiff_chunk(b"JUNK", &[0; 100]));
            bytes.extend(aiff_chunk(b"COMM", common));
            bytes.extend(aiff_chunk(b"SSND", &[7; 500]));
            bytes
        };
        let flac_claiming = |packed: [u8; 3]| {
            let mut info = [0; 34];
            info[10..13].copy_from_slice(&packed);
            let mut bytes = b"fLaC".to_vec();
            bytes.extend(flac_block(1, &[0; 100]));
            bytes.extend(flac_block(0x80, &info));
            bytes.extend([7; 500]);
            bytes
        };
        let cases: [(Vec<u8>, MakeFilter, usize, &str); 6] = [
            (
                wave_claiming(&[1, 0, 65, 0, 0x44, 0xAC, 0, 0, 0, 0, 0, 0, 130, 0, 16, 0]),
                wave,
                12 + 108,
                TOO_MANY,
            ),
            (
                wave_claiming(&[3, 0, 2, 0, 0x01, 0xB8, 0x0B, 0, 0, 0, 0, 0, 8, 0, 32, 0]),
                wave,
                12 + 108,
                TOO_FAST,
            ),
            (
                aiff_claiming(&[
                    0, 65, 0, 0, 0, 4, 0, 16, 0x40, 0x0E, 0xAC, 0x44, 0, 0, 0, 0, 0, 0,
                ]),
                aiff,
                12 + 108,
                TOO_MANY,
            ),
            (
                aiff_claiming(&[
                    0, 2, 0, 0, 0, 4, 0, 16, 0x40, 0x12, 0xBB, 0x80, 0x10, 0, 0, 0, 0, 0,
                ]),
                aiff,
                12 + 108,
                TOO_FAST,
            ),
            // FLAC has no room for more than eight channels.
            (flac_claiming([0xBB, 0x80, 0x12]), flac, 4 + 104, TOO_FAST),
            // The stream starts behind that of a film it comes with, and
            // every byte of the film's page is handed out first.
            (
                [
                    ogg_page(0x02, &[42], b"\x80theora"),
                    ogg_page(0x02, &[30], b"\x01vorbis\0\0\0\0\x41\x44\xAC\0\0"),
                    ogg_page(0, &[200], b""),
                ]
                .concat(),
                ogg,
                OGG_HEADER_LEN + 1 + 42,
                TOO_MANY,
            ),
        ];
        for (bytes, filter, claim_at, refusal) in cases {
            let (seen, shown) = refused(&bytes, filter);
            assert_eq!(shown, refusal);
            assert!(seen <= claim_at, "{refusal}: {seen} bytes were read");
        }

        let film = ogg_page(0x02, &[42], b"\x80theora");
        let fast = ogg_page(0x02, &[30], b"\x01vorbis\0\0\0\0\x02\x01\xB8\x0B\0");
        let bytes = [film.clone(), fast].concat();
        assert_eq!(refused(&bytes, ogg), (film.len(), TOO_FAST.to_owned()));
    }

    #[test]
    fn only_claims_over_the_limits_are_refused() {
        assert!(check_claim(MAX_CHANNELS, MAX_SAMPLE_RATE).is_ok());
        assert!(check_claim(0, 0).is_ok());
        assert!(check_claim(MAX_CHANNELS + 1, 44_100).is_err());
        assert!(check_claim(2, MAX_SAMPLE_RATE + 1).is_err());

        // A stream that is not Vorbis, and a later page of one that is.
        let pages = [
            ogg_page(0x02, &[30], b"\x7FFLAC\0\0\0\0\xFF\xFF\xFF\xFF\xFF"),
            ogg_page(0, &[30], b"\x01vorbis\0\0\0\0\xFF\xFF\xFF\xFF\xFF"),
        ]
        .concat();
        assert!(seen(&pages, ogg) == pages);
    }

    #[test]
    fn an_aiff_sample_rate_is_read_in_whole_hertz() {
        let rate = |head: [u8; 2], mantissa: u64| {
            let mut bytes = [0; 10];
            bytes[..2].copy_from_slice(&head);
            bytes[2..].copy_from_slice(&mantissa.to_be_bytes());
            aiff_sample_rate(&bytes)
        };
        assert_eq!(rate([0x40, 0x0E], 0xAC44 << 48), 44_100);
        assert_eq!(rate([0x40, 0x12], 0xBB801 << 44), 768_001);
        // 44,100.9 Hz, and the same with a mantissa that does not start
        // with a one.
        assert_eq!(rate([0x40, 0x0E], 0xAC44_E666 << 32), 44_100);
        assert_eq!(rate([0x40, 0x0F], 0xAC44_E666 << 31), 44_100);
        // A half, and nothing.
        assert_eq!(rate([0x3F, 0xFE], 1 << 63), 0);
        assert_eq!(rate([0, 0], 0), 0);
        // Two to the power of 40 and the highest number there is.
        assert_eq!(rate([0x40, 0x27], 1 << 63), u32::MAX);
        assert_eq!(rate([0x7F, 0xFE], u64::MAX), u32::MAX);
        // Below zero, and infinity.
        assert_eq!(rate([0xC0, 0x0E], 0xAC44 << 48), 0);
        assert_eq!(rate([0x7F, 0xFF], 1 << 63), 0);
    }
}
