use std::fs::{self, File};
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use windfall_core::AudioBuffer;

use crate::encoder::AudioTags;
use crate::error::CodecError;

/// Seed the dither starts from unless [`WavWriter::with_dither_seed`] sets
/// another. It is fixed so that exporting the same audio twice gives the same
/// bytes.
pub const DEFAULT_DITHER_SEED: u64 = 0x5769_6E64_6661_6C6C;

/// Samples converted between writes to the file.
const SAMPLES_PER_CHUNK: usize = 16 * 1024;

const FORMAT_PCM: u16 = 0x0001;
const FORMAT_IEEE_FLOAT: u16 = 0x0003;
const FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// The fixed tail every `KSDATAFORMAT_SUBTYPE` GUID shares. The format tag
/// goes in front of it.
const SUBFORMAT_GUID_TAIL: [u8; 14] = [
    0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];

/// Keeps temporary file names apart when one process writes several files
/// with the same name at once.
static NEXT_TEMP_ID: AtomicU32 = AtomicU32::new(0);

/// How samples are stored in a WAV file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WavSampleFormat {
    /// 16-bit integers, dithered.
    Int16,
    /// 24-bit integers, dithered.
    Int24,
    /// 32-bit floats, written exactly as given.
    Float32,
}

impl WavSampleFormat {
    fn bytes_per_sample(self) -> u16 {
        match self {
            Self::Int16 => 2,
            Self::Int24 => 3,
            Self::Float32 => 4,
        }
    }

    fn is_float(self) -> bool {
        self == Self::Float32
    }
}

/// Writes a whole buffer to a WAV file. See [`WavWriter`] for how samples are
/// converted and how the file is put in place.
pub fn write_wav(
    path: impl AsRef<Path>,
    buffer: &AudioBuffer,
    format: WavSampleFormat,
) -> Result<(), CodecError> {
    let mut writer = WavWriter::create(path, buffer.sample_rate(), buffer.channels(), format)?;
    writer.write(buffer.samples())?;
    writer.finalize()
}

/// Writes a WAV file a block at a time, for exports too long to hold in
/// memory.
///
/// Integer formats are dithered with triangular (TPDF) noise from a seeded
/// generator, so the same input always gives the same file. Values beyond
/// full scale are clipped. Float output is written unchanged.
///
/// The audio goes to a temporary file beside the destination and takes the
/// destination's name only in [`finalize`](Self::finalize). If the writer is
/// dropped first, or anything fails, the temporary file is removed and
/// whatever was at the destination before is left untouched.
#[derive(Debug)]
pub struct WavWriter {
    /// `None` once the file has been handed over in `finalize`.
    file: Option<BufWriter<File>>,
    temp_path: PathBuf,
    final_path: PathBuf,
    layout: Layout,
    data_bytes: u64,
    dither: Dither,
    scratch: Vec<u8>,
    /// Set while a write is under way and left set if it fails, because the
    /// file then holds part of a block.
    broken: bool,
    /// Set once the file has its final name and nothing is left to clean up.
    placed: bool,
}

impl WavWriter {
    /// Starts a WAV file that will appear at `path` when finalized.
    pub fn create(
        path: impl AsRef<Path>,
        sample_rate: u32,
        channels: u16,
        format: WavSampleFormat,
    ) -> Result<Self, CodecError> {
        Self::create_with_tags(path, sample_rate, channels, format, &AudioTags::default())
    }

    /// Starts a WAV file with a LIST/INFO chunk for nonempty project fields.
    pub fn create_with_tags(
        path: impl AsRef<Path>,
        sample_rate: u32,
        channels: u16,
        format: WavSampleFormat,
        tags: &AudioTags<'_>,
    ) -> Result<Self, CodecError> {
        Self::create_with_metadata(
            path,
            sample_rate,
            channels,
            format,
            tags,
            &crate::WavMetadata::default(),
        )
    }

    /// Starts a WAV with bounded cue/loop/slice/tempo metadata before audio.
    pub fn create_with_metadata(
        path: impl AsRef<Path>,
        sample_rate: u32,
        channels: u16,
        format: WavSampleFormat,
        tags: &AudioTags<'_>,
        metadata: &crate::WavMetadata,
    ) -> Result<Self, CodecError> {
        let mut layout = Layout::new(sample_rate, channels, format)?;
        layout.marker_chunks = metadata.chunks(sample_rate)?;
        layout.metadata = metadata.clone();
        layout.info_chunk = info_chunk(tags, layout.max_data_bytes())?;
        let final_path = path.as_ref().to_path_buf();
        let name = final_path.file_name().ok_or_else(|| {
            CodecError::InvalidInput(format!("{} is not a file name", final_path.display()))
        })?;

        // The name is unique among running processes, so a file already there
        // is left over from a crashed export and is safe to replace.
        let mut temp_name = std::ffi::OsString::from(".");
        temp_name.push(name);
        temp_name.push(format!(
            ".{}-{}.tmp",
            std::process::id(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let temp_path = final_path.with_file_name(temp_name);

        let mut file = BufWriter::with_capacity(64 * 1024, File::create(&temp_path)?);
        // Room for the header. The real sizes are filled in by `finalize`.
        let reserved = file.write_all(&layout.header(0));
        // The writer is built before the result is checked so that dropping
        // it on failure removes the temporary file.
        let writer = Self {
            file: Some(file),
            temp_path,
            final_path,
            layout,
            data_bytes: 0,
            dither: Dither::new(DEFAULT_DITHER_SEED),
            scratch: Vec::new(),
            broken: false,
            placed: false,
        };
        reserved?;
        Ok(writer)
    }

    /// Restarts the dither from `seed`. Call it before the first write.
    pub fn with_dither_seed(mut self, seed: u64) -> Self {
        self.dither = Dither::new(seed);
        self
    }

    /// Appends interleaved samples. The length must be a whole number of
    /// frames.
    ///
    /// Returns [`CodecError::WavTooLarge`] without writing anything if the
    /// block would take the file past 4 GB; the audio written so far can
    /// still be finalized.
    pub fn write(&mut self, interleaved: &[f32]) -> Result<(), CodecError> {
        if self.broken {
            return Err(Self::broken_error());
        }
        let file = self.file.as_mut().ok_or_else(Self::broken_error)?;
        if !interleaved
            .len()
            .is_multiple_of(usize::from(self.layout.channels))
        {
            return Err(CodecError::InvalidInput(format!(
                "{} samples do not make whole frames of {} channels",
                interleaved.len(),
                self.layout.channels
            )));
        }

        let bytes_per_sample = u64::from(self.layout.format.bytes_per_sample());
        let data_bytes = self.data_bytes + interleaved.len() as u64 * bytes_per_sample;
        if data_bytes > self.layout.max_data_bytes() {
            return Err(CodecError::WavTooLarge);
        }

        self.broken = true;
        for chunk in interleaved.chunks(SAMPLES_PER_CHUNK) {
            self.scratch.clear();
            match self.layout.format {
                WavSampleFormat::Int16 => {
                    for &sample in chunk {
                        let value = self.dither.quantize(sample, 32_768.0) as i16;
                        self.scratch.extend_from_slice(&value.to_le_bytes());
                    }
                }
                WavSampleFormat::Int24 => {
                    for &sample in chunk {
                        let value = self.dither.quantize(sample, 8_388_608.0);
                        self.scratch.extend_from_slice(&value.to_le_bytes()[..3]);
                    }
                }
                WavSampleFormat::Float32 => {
                    for &sample in chunk {
                        self.scratch.extend_from_slice(&sample.to_le_bytes());
                    }
                }
            }
            file.write_all(&self.scratch)?;
        }
        self.broken = false;
        self.data_bytes = data_bytes;
        Ok(())
    }

    /// Adds metadata discovered while rendering, without moving streamed PCM.
    /// RIFF permits cue/smpl/LIST/acid chunks after the data chunk.
    pub fn set_rendered_metadata(&mut self, metadata: &crate::WavMetadata) -> Result<(), CodecError> {
        if !self.layout.marker_chunks.is_empty() {
            return Err(CodecError::InvalidInput("WAV metadata was already supplied before rendering".into()));
        }
        metadata.validate(Some(self.data_bytes / u64::from(self.layout.block_align)))?;
        let chunks = metadata.chunks(self.layout.sample_rate)?;
        let overhead = self.layout.header(0).len() as u64 - 8 + 1 + chunks.len() as u64;
        if overhead + self.data_bytes > u64::from(u32::MAX) {
            return Err(CodecError::WavTooLarge);
        }
        self.layout.trailing_chunks = chunks;
        self.layout.metadata = metadata.clone();
        Ok(())
    }

    /// Completes the header and publishes the file.
    pub fn finalize(mut self) -> Result<(), CodecError> {
        if self.broken {
            return Err(Self::broken_error());
        }
        self.layout
            .metadata
            .validate(Some(self.data_bytes / u64::from(self.layout.block_align)))?;
        let mut buffered = self.file.take().ok_or_else(Self::broken_error)?;
        // Chunks are word-aligned: odd-length data is followed by a pad byte
        // that the chunk size does not count.
        if !self.data_bytes.is_multiple_of(2) {
            buffered.write_all(&[0])?;
        }
        buffered.write_all(&self.layout.trailing_chunks)?;
        let mut file = buffered.into_inner().map_err(|error| error.into_error())?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&self.layout.header(self.data_bytes))?;
        file.sync_all()?;
        drop(file);

        fs::rename(&self.temp_path, &self.final_path)?;
        self.placed = true;
        Ok(())
    }

    fn broken_error() -> CodecError {
        CodecError::InvalidInput("an earlier write to this file failed".to_owned())
    }
}

impl Drop for WavWriter {
    fn drop(&mut self) {
        if self.placed {
            return;
        }
        // The file has to be closed before Windows will delete it.
        self.file = None;
        let _ = fs::remove_file(&self.temp_path);
    }
}

/// Refuses what a WAV file cannot hold, without creating one.
pub(crate) fn check_layout(
    sample_rate: u32,
    channels: u16,
    format: WavSampleFormat,
) -> Result<(), CodecError> {
    Layout::new(sample_rate, channels, format).map(drop)
}

/// The parts of the header that are known before any audio is written.
#[derive(Debug, Clone)]
struct Layout {
    sample_rate: u32,
    channels: u16,
    format: WavSampleFormat,
    block_align: u16,
    byte_rate: u32,
    info_chunk: Vec<u8>,
    marker_chunks: Vec<u8>,
    trailing_chunks: Vec<u8>,
    metadata: crate::WavMetadata,
}

impl Layout {
    fn new(sample_rate: u32, channels: u16, format: WavSampleFormat) -> Result<Self, CodecError> {
        if sample_rate == 0 {
            return Err(CodecError::InvalidInput(
                "the sample rate is zero".to_owned(),
            ));
        }
        if channels == 0 {
            return Err(CodecError::InvalidInput("there are no channels".to_owned()));
        }
        let block_align = channels.checked_mul(format.bytes_per_sample());
        let byte_rate = block_align.and_then(|align| sample_rate.checked_mul(u32::from(align)));
        let (Some(block_align), Some(byte_rate)) = (block_align, byte_rate) else {
            return Err(CodecError::InvalidInput(format!(
                "{channels} channels at {sample_rate} Hz do not fit a WAV header"
            )));
        };
        Ok(Self {
            sample_rate,
            channels,
            format,
            block_align,
            byte_rate,
            info_chunk: Vec::new(),
            marker_chunks: Vec::new(),
            trailing_chunks: Vec::new(),
            metadata: crate::WavMetadata::default(),
        })
    }

    /// Plain headers cover mono and stereo up to 16 bits. Anything else needs
    /// the extensible header to say where the channels go and how many bits
    /// are valid.
    fn is_extensible(&self) -> bool {
        self.channels > 2 || self.format == WavSampleFormat::Int24
    }

    /// Speaker positions for the usual layouts, in WAV channel order. Zero
    /// means the channels have no assigned positions.
    fn channel_mask(&self) -> u32 {
        match self.channels {
            1 => 0x4,
            2 => 0x3,
            3 => 0x7,
            4 => 0x33,
            5 => 0x37,
            6 => 0x3F,
            7 => 0x13F,
            8 => 0x63F,
            _ => 0,
        }
    }

    /// The most audio the file can hold while every size still fits the
    /// header's 32-bit fields.
    fn max_data_bytes(&self) -> u64 {
        // The RIFF size counts everything after its own 8 bytes, including
        // the pad byte after odd-length data.
        let overhead = self.header(0).len() as u64 - 8 + 1 + self.trailing_chunks.len() as u64;
        u64::from(u32::MAX) - overhead
    }

    /// Everything in the file before the samples. `data_bytes` must not
    /// exceed [`max_data_bytes`](Self::max_data_bytes).
    fn header(&self, data_bytes: u64) -> Vec<u8> {
        let bits = self.format.bytes_per_sample() * 8;
        let tag = if self.format.is_float() {
            FORMAT_IEEE_FLOAT
        } else {
            FORMAT_PCM
        };

        let mut fmt = Vec::with_capacity(40);
        let outer_tag = if self.is_extensible() {
            FORMAT_EXTENSIBLE
        } else {
            tag
        };
        fmt.extend_from_slice(&outer_tag.to_le_bytes());
        fmt.extend_from_slice(&self.channels.to_le_bytes());
        fmt.extend_from_slice(&self.sample_rate.to_le_bytes());
        fmt.extend_from_slice(&self.byte_rate.to_le_bytes());
        fmt.extend_from_slice(&self.block_align.to_le_bytes());
        fmt.extend_from_slice(&bits.to_le_bytes());
        if self.is_extensible() {
            fmt.extend_from_slice(&22_u16.to_le_bytes());
            fmt.extend_from_slice(&bits.to_le_bytes());
            fmt.extend_from_slice(&self.channel_mask().to_le_bytes());
            fmt.extend_from_slice(&tag.to_le_bytes());
            fmt.extend_from_slice(&SUBFORMAT_GUID_TAIL);
        } else if self.format.is_float() {
            // Every format but plain PCM carries an extension size, here zero.
            fmt.extend_from_slice(&0_u16.to_le_bytes());
        }

        let mut chunks = Vec::with_capacity(80);
        chunks.extend_from_slice(b"WAVEfmt ");
        chunks.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        chunks.extend_from_slice(&fmt);
        if self.format.is_float() {
            // Non-PCM formats must state their length in frames.
            let frames = data_bytes / u64::from(self.block_align);
            chunks.extend_from_slice(b"fact");
            chunks.extend_from_slice(&4_u32.to_le_bytes());
            chunks.extend_from_slice(&(frames as u32).to_le_bytes());
        }
        chunks.extend_from_slice(&self.info_chunk);
        chunks.extend_from_slice(&self.marker_chunks);
        chunks.extend_from_slice(b"data");
        chunks.extend_from_slice(&(data_bytes as u32).to_le_bytes());

        let riff_size = chunks.len() as u64 + data_bytes + data_bytes % 2 + self.trailing_chunks.len() as u64;
        let mut header = Vec::with_capacity(chunks.len() + 8);
        header.extend_from_slice(b"RIFF");
        header.extend_from_slice(&(riff_size as u32).to_le_bytes());
        header.extend_from_slice(&chunks);
        header
    }
}

/// INFO strings include a terminating NUL; sizes exclude word-alignment padding.
fn info_chunk(tags: &AudioTags<'_>, max_bytes: u64) -> Result<Vec<u8>, CodecError> {
    let fields = [
        (b"INAM", tags.title),
        (b"IART", tags.author),
        (b"IGNR", tags.genre),
        (b"ICMT", tags.comments),
    ];
    if fields.iter().all(|(_, value)| value.is_empty()) {
        return Ok(Vec::new());
    }
    let bytes =
        fields
            .iter()
            .filter(|(_, value)| !value.is_empty())
            .fold(12_u64, |total, (_, value)| {
                let size = value.len() as u64 + 1;
                total + 8 + size + size % 2
            });
    if bytes > max_bytes {
        return Err(CodecError::WavTooLarge);
    }
    let mut chunk = Vec::with_capacity(bytes as usize);
    chunk.extend_from_slice(b"LIST");
    chunk.extend_from_slice(&((bytes - 8) as u32).to_le_bytes());
    chunk.extend_from_slice(b"INFO");
    for (id, value) in fields {
        if value.is_empty() {
            continue;
        }
        let size = value.len() as u32 + 1;
        chunk.extend_from_slice(id);
        chunk.extend_from_slice(&size.to_le_bytes());
        chunk.extend_from_slice(value.as_bytes());
        chunk.push(0);
        if !size.is_multiple_of(2) {
            chunk.push(0);
        }
    }
    Ok(chunk)
}

/// Triangular dither from a SplitMix64 generator: no clock and no operating
/// system randomness, so a given seed always gives the same noise.
#[derive(Debug, Clone)]
pub(crate) struct Dither {
    state: u64,
}

impl Dither {
    pub(crate) fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^ (mixed >> 31)
    }

    /// Noise between -1 and 1 quantization steps with a triangular
    /// distribution: the difference of two uniform values.
    fn noise(&mut self) -> f64 {
        let bits = self.next_u64();
        let first = (bits >> 32) as f64;
        let second = (bits & 0xFFFF_FFFF) as f64;
        (first - second) / 4_294_967_296.0
    }

    /// Scales a sample to integer steps, adds dither, rounds to the nearest
    /// step and clips to the range `-full_scale..full_scale`. The work is
    /// done in 64-bit so that 24-bit output keeps its last bit. NaN becomes
    /// silence.
    pub(crate) fn quantize(&mut self, sample: f32, full_scale: f64) -> i32 {
        let dithered = f64::from(sample) * full_scale + self.noise();
        (dithered + 0.5)
            .floor()
            .clamp(-full_scale, full_scale - 1.0) as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16_at(bytes: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([bytes[at], bytes[at + 1]])
    }

    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
    }

    #[test]
    fn dither_noise_is_triangular() {
        let mut dither = Dither::new(1);
        let count = 200_000;
        let (mut sum, mut sum_of_squares) = (0.0, 0.0);
        for _ in 0..count {
            let noise = dither.noise();
            assert!(noise > -1.0 && noise < 1.0);
            sum += noise;
            sum_of_squares += noise * noise;
        }
        let mean = sum / f64::from(count);
        let variance = sum_of_squares / f64::from(count) - mean * mean;
        assert!(mean.abs() < 0.01, "mean {mean}");
        // A triangular distribution over -1..1 has variance 1/6.
        assert!((variance - 1.0 / 6.0).abs() < 0.01, "variance {variance}");
    }

    #[test]
    fn dither_keeps_levels_below_one_step() {
        // A constant 0.3 of a step would round to silence without dither.
        // With it, the average of the output is still 0.3 of a step.
        let mut dither = Dither::new(7);
        let sample = 0.3 / 32_768.0;
        let count = 200_000;
        let sum: i64 = (0..count)
            .map(|_| i64::from(dither.quantize(sample, 32_768.0)))
            .sum();
        let mean = sum as f64 / f64::from(count);
        assert!((mean - 0.3).abs() < 0.01, "mean {mean}");
    }

    #[test]
    fn quantizing_stays_within_a_step_and_a_half() {
        let mut dither = Dither::new(3);
        for step in -2_000..2_000 {
            let sample = step as f32 / 2_048.0 * 0.99;
            let exact = f64::from(sample) * 32_768.0;
            let value = f64::from(dither.quantize(sample, 32_768.0));
            assert!((value - exact).abs() < 1.5, "{sample} became {value}");
        }
    }

    #[test]
    fn quantizing_clips_at_full_scale() {
        let mut dither = Dither::new(3);
        for _ in 0..1_000 {
            assert_eq!(dither.quantize(1.0, 32_768.0), 32_767);
            assert_eq!(dither.quantize(4.0, 32_768.0), 32_767);
            assert_eq!(dither.quantize(-4.0, 32_768.0), -32_768);
            assert_eq!(dither.quantize(f32::INFINITY, 8_388_608.0), 8_388_607);
            assert_eq!(dither.quantize(f32::NEG_INFINITY, 8_388_608.0), -8_388_608);
            assert_eq!(dither.quantize(f32::NAN, 32_768.0), 0);
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_noise() {
        let mut first = Dither::new(42);
        let mut second = Dither::new(42);
        let mut other = Dither::new(43);
        let mut differs = false;
        for _ in 0..1_000 {
            let value = first.next_u64();
            assert_eq!(value, second.next_u64());
            differs |= value != other.next_u64();
        }
        assert!(differs);
    }

    #[test]
    fn sixteen_bit_stereo_gets_the_plain_44_byte_header() {
        let layout = Layout::new(44_100, 2, WavSampleFormat::Int16).unwrap();
        let header = layout.header(400);
        assert_eq!(header.len(), 44);
        assert_eq!(&header[0..4], b"RIFF");
        assert_eq!(u32_at(&header, 4), 36 + 400);
        assert_eq!(&header[8..16], b"WAVEfmt ");
        assert_eq!(u32_at(&header, 16), 16);
        assert_eq!(u16_at(&header, 20), FORMAT_PCM);
        assert_eq!(u16_at(&header, 22), 2);
        assert_eq!(u32_at(&header, 24), 44_100);
        assert_eq!(u32_at(&header, 28), 44_100 * 4);
        assert_eq!(u16_at(&header, 32), 4);
        assert_eq!(u16_at(&header, 34), 16);
        assert_eq!(&header[36..40], b"data");
        assert_eq!(u32_at(&header, 40), 400);
    }

    #[test]
    fn float_headers_state_the_frame_count() {
        let layout = Layout::new(48_000, 2, WavSampleFormat::Float32).unwrap();
        let header = layout.header(800);
        assert_eq!(u16_at(&header, 20), FORMAT_IEEE_FLOAT);
        assert_eq!(u32_at(&header, 16), 18);
        assert_eq!(&header[38..42], b"fact");
        assert_eq!(u32_at(&header, 46), 100);
        assert_eq!(&header[50..54], b"data");
        assert_eq!(u32_at(&header, 4) as usize, header.len() - 8 + 800);
    }

    #[test]
    fn surround_and_24_bit_use_the_extensible_header() {
        let surround = Layout::new(48_000, 6, WavSampleFormat::Int16)
            .unwrap()
            .header(0);
        assert_eq!(u16_at(&surround, 20), FORMAT_EXTENSIBLE);
        assert_eq!(u32_at(&surround, 16), 40);
        assert_eq!(u32_at(&surround, 40), 0x3F);
        assert_eq!(u16_at(&surround, 44), FORMAT_PCM);

        let deep = Layout::new(48_000, 2, WavSampleFormat::Int24)
            .unwrap()
            .header(0);
        assert_eq!(u16_at(&deep, 20), FORMAT_EXTENSIBLE);
        assert_eq!(u16_at(&deep, 34), 24);
        assert_eq!(u16_at(&deep, 38), 24);
        assert_eq!(u32_at(&deep, 40), 0x3);
    }

    #[test]
    fn a_file_that_would_pass_4_gb_is_refused_with_a_clear_error() {
        let path = std::env::temp_dir().join(format!(
            "windfall-codec-4gb-limit-{}.wav",
            std::process::id()
        ));
        let mut writer = WavWriter::create(&path, 48_000, 2, WavSampleFormat::Float32).unwrap();
        let temp_path = writer.temp_path.clone();
        assert!(temp_path.exists());

        // Stand in for 4 GB already written, one frame short of the limit.
        let limit = writer.layout.max_data_bytes();
        assert!(limit < u64::from(u32::MAX));
        writer.data_bytes = limit - limit % 8 - 8;
        writer.write(&[0.0, 0.0]).unwrap();

        let error = writer.write(&[0.0, 0.0]).unwrap_err();
        assert!(matches!(error, CodecError::WavTooLarge));
        assert!(error.to_string().contains("4 GB"));

        // Every size in the largest header still fits its 32-bit field.
        let header = writer.layout.header(limit);
        assert_eq!(u64::from(u32_at(&header, header.len() - 4)), limit);
        assert_eq!(
            u64::from(u32_at(&header, 4)),
            header.len() as u64 - 8 + limit + limit % 2
        );

        drop(writer);
        assert!(!temp_path.exists());
        assert!(!path.exists());
    }
}
