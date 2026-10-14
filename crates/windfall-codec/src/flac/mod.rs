//! Writing FLAC: lossless, about half the size of a WAV file.
//!
//! The encoder is Windfall's own. A stream is cut into blocks of 4,096
//! samples. Each channel of a block is stored as the best of four things:
//! one value, when the channel is constant; a fixed polynomial predictor
//! or a predictor fitted to the block, with what the predictor leaves
//! Rice-coded; or the samples as they are, when nothing else is smaller.
//! Two channels may be stored as their sum and difference instead. The
//! file states an MD5 sum of the audio and every frame a checksum, so a
//! decoder knows it got back exactly what went in.

mod bits;
mod predict;
mod rice;

use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use windfall_core::AudioBuffer;

use self::bits::{BitWriter, crc8, crc16};
use self::predict::{Lpc, MAX_FIXED_ORDER, fixed_residual, window};
use self::rice::{RiceLimits, RicePlan, RiceScratch};
use crate::atomic::AtomicFile;
use crate::encoder::AudioTags;
use crate::error::CodecError;
use crate::md5::Md5;
use crate::wav::{DEFAULT_DITHER_SEED, Dither};

/// The compression level a FLAC file gets unless another is asked for. It
/// is the level the reference encoder uses by default as well.
pub const DEFAULT_FLAC_LEVEL: u8 = 5;

/// The highest compression level. Every level is lossless; a higher one
/// takes longer and gives a slightly smaller file.
pub const MAX_FLAC_LEVEL: u8 = 8;

/// The highest sample rate a FLAC file can state.
pub const MAX_FLAC_SAMPLE_RATE: u32 = 655_350;

/// The most channels a FLAC file can hold.
pub const MAX_FLAC_CHANNELS: u16 = 8;

/// Samples of each channel in a frame, except the last of a file.
const BLOCK: usize = 4096;

/// The name of the encoder, which a FLAC file carries in its comment block.
const VENDOR: &[u8] = b"Windfall";

/// Where the stream's description starts: after the `fLaC` marker and the
/// four bytes that head the block.
const STREAM_INFO_AT: u64 = 8;

/// How samples are stored in a FLAC file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlacBitDepth {
    /// 16-bit integers, dithered.
    Int16,
    /// 24-bit integers, dithered.
    Int24,
}

impl FlacBitDepth {
    fn bits(self) -> u32 {
        match self {
            Self::Int16 => 16,
            Self::Int24 => 24,
        }
    }

    fn full_scale(self) -> f64 {
        match self {
            Self::Int16 => 32_768.0,
            Self::Int24 => 8_388_608.0,
        }
    }
}

/// Writes a whole buffer to a FLAC file. See [`FlacWriter`].
pub fn write_flac(
    path: impl AsRef<Path>,
    buffer: &AudioBuffer,
    depth: FlacBitDepth,
    level: u8,
) -> Result<(), CodecError> {
    let (rate, channels) = (buffer.sample_rate(), buffer.channels());
    let mut writer = FlacWriter::create(path, rate, channels, depth, level)?;
    writer.write(buffer.samples())?;
    writer.finalize()
}

/// What a compression level asks of the encoder.
#[derive(Debug, Clone, Copy)]
struct Effort {
    /// The highest order of fitted predictor to try. Zero tries the fixed
    /// predictors only.
    lpc_order: usize,
    /// Tries every order up to `lpc_order` instead of the likeliest.
    every_order: bool,
    /// Tries storing two channels as left and difference, difference and
    /// right, and sum and difference, and keeps the smallest.
    stereo: bool,
    /// The most partitions the Rice coder may cut a block into, as a power
    /// of two.
    rice_order: u32,
}

impl Effort {
    fn of(level: u8) -> Self {
        let (lpc_order, stereo, rice_order) = match level {
            0 => (0, false, 3),
            1 => (0, true, 3),
            2 => (0, true, 5),
            3 => (6, false, 4),
            4 => (8, true, 4),
            5 => (8, true, 5),
            6 => (8, true, 6),
            7 => (12, true, 6),
            _ => (12, true, 8),
        };
        Self {
            lpc_order,
            every_order: level >= MAX_FLAC_LEVEL,
            stereo,
            rice_order,
        }
    }
}

/// Writes a FLAC file a block at a time, for exports too long to hold in
/// memory.
///
/// Samples are turned into integers the way [`WavWriter`] does it, with
/// the same dither from the same seed, so a FLAC file and a WAV file of
/// the same audio and bit depth hold the same numbers, and the same input
/// always gives the same file. FLAC stores those integers exactly.
///
/// The audio goes to a temporary file beside the destination and takes the
/// destination's name only in [`finalize`](Self::finalize). If the writer is
/// dropped first, or anything fails, the temporary file is removed and
/// whatever was at the destination before is left untouched.
///
/// [`WavWriter`]: crate::WavWriter
#[derive(Debug)]
pub struct FlacWriter {
    file: AtomicFile,
    sample_rate: u32,
    channels: usize,
    depth: FlacBitDepth,
    effort: Effort,
    dither: Dither,
    /// The samples of each channel that do not fill a block yet.
    pending: Vec<Vec<i32>>,
    md5: Md5,
    md5_bytes: Vec<u8>,
    frame: BitWriter,
    work: Workbench,
    /// Frames written so far.
    frames: u32,
    /// Samples of each channel written so far.
    samples: u64,
    /// The smallest and largest frame so far, in bytes.
    frame_bytes: Option<(usize, usize)>,
    /// Set while a write is under way and left set if it fails, because the
    /// file then holds part of a frame.
    broken: bool,
}

impl FlacWriter {
    /// Starts a FLAC file that will appear at `path` when finalized.
    /// `level` is the compression level, 0 to [`MAX_FLAC_LEVEL`].
    pub fn create(
        path: impl AsRef<Path>,
        sample_rate: u32,
        channels: u16,
        depth: FlacBitDepth,
        level: u8,
    ) -> Result<Self, CodecError> {
        Self::create_with_tags(
            path,
            sample_rate,
            channels,
            depth,
            level,
            &AudioTags::default(),
        )
    }

    /// Starts a FLAC file with Vorbis comments for nonempty project fields.
    pub fn create_with_tags(
        path: impl AsRef<Path>,
        sample_rate: u32,
        channels: u16,
        depth: FlacBitDepth,
        level: u8,
        tags: &AudioTags<'_>,
    ) -> Result<Self, CodecError> {
        check_layout(sample_rate, channels, level)?;
        let comments = vorbis_comments(tags)?;
        let mut file = AtomicFile::create(path.as_ref())?;
        let mut head = Vec::with_capacity(64);
        head.extend_from_slice(b"fLaC");
        // The stream's description, 34 bytes that `finalize` fills in.
        head.extend_from_slice(&[0, 0, 0, 34]);
        head.extend_from_slice(&[0; 34]);
        // The comment block retains the encoder vendor and is the last
        // metadata block before the audio.
        let comment_bytes = comments.len() as u32;
        head.push(0x80 | 4);
        head.extend_from_slice(&comment_bytes.to_be_bytes()[1..]);
        head.extend_from_slice(&comments);
        file.write_all(&head)?;

        let channels = usize::from(channels);
        Ok(Self {
            file,
            sample_rate,
            channels,
            depth,
            effort: Effort::of(level),
            dither: Dither::new(DEFAULT_DITHER_SEED),
            pending: (0..channels).map(|_| Vec::with_capacity(BLOCK)).collect(),
            md5: Md5::new(),
            md5_bytes: Vec::new(),
            frame: BitWriter::default(),
            work: Workbench::default(),
            frames: 0,
            samples: 0,
            frame_bytes: None,
            broken: false,
        })
    }

    /// Restarts the dither from `seed`. Call it before the first write.
    pub fn with_dither_seed(mut self, seed: u64) -> Self {
        self.dither = Dither::new(seed);
        self
    }

    /// Appends interleaved samples. The length must be a whole number of
    /// frames.
    pub fn write(&mut self, interleaved: &[f32]) -> Result<(), CodecError> {
        if self.broken {
            return Err(broken_error());
        }
        if !interleaved.len().is_multiple_of(self.channels) {
            return Err(CodecError::InvalidInput(
                windfall_core::notice!(
                    "codec.partialFrames",
                    samples = interleaved.len(),
                    channels = self.channels
                )
                .to_string(),
            ));
        }
        self.broken = true;
        let full_scale = self.depth.full_scale();
        for frame in interleaved.chunks_exact(self.channels) {
            for (channel, &sample) in self.pending.iter_mut().zip(frame) {
                channel.push(self.dither.quantize(sample, full_scale));
            }
            if self.pending[0].len() == BLOCK {
                self.write_frame()?;
            }
        }
        self.broken = false;
        Ok(())
    }

    /// Writes the last block, completes the header and moves the file to
    /// its destination, replacing any file already there.
    pub fn finalize(mut self) -> Result<(), CodecError> {
        if self.broken {
            return Err(broken_error());
        }
        if !self.pending[0].is_empty() {
            self.write_frame()?;
        }
        let info = self.stream_info();
        self.file.place(|file| {
            file.seek(SeekFrom::Start(STREAM_INFO_AT))?;
            file.write_all(&info)
        })?;
        Ok(())
    }

    /// The 34 bytes that describe the stream.
    fn stream_info(&self) -> [u8; 34] {
        let (smallest, largest) = self.frame_bytes.unwrap_or((0, 0));
        // A size that does not fit its field is given as zero: not known.
        let sized = |bytes: usize| u32::try_from(bytes).ok().filter(|&bytes| bytes < 1 << 24);
        let samples = if self.samples < 1 << 36 {
            self.samples
        } else {
            0
        };
        let mut info = [0; 34];
        info[0..2].copy_from_slice(&(BLOCK as u16).to_be_bytes());
        info[2..4].copy_from_slice(&(BLOCK as u16).to_be_bytes());
        info[4..7].copy_from_slice(&sized(smallest).unwrap_or(0).to_be_bytes()[1..]);
        info[7..10].copy_from_slice(&sized(largest).unwrap_or(0).to_be_bytes()[1..]);
        // 20 bits of sample rate, 3 of channels, 5 of bit depth and 36 of
        // length.
        let packed = (u64::from(self.sample_rate) << 44)
            | ((self.channels as u64 - 1) << 41)
            | (u64::from(self.depth.bits() - 1) << 36)
            | samples;
        info[10..18].copy_from_slice(&packed.to_be_bytes());
        info[18..34].copy_from_slice(&self.md5.clone().finish());
        info
    }

    /// Encodes and writes what is pending as one frame, and empties it.
    fn write_frame(&mut self) -> Result<(), CodecError> {
        let count = self.pending[0].len();
        // A frame carries its number in 31 bits.
        if self.frames > i32::MAX as u32 {
            return Err(CodecError::InvalidInput(
                "the audio is too long for a FLAC file".to_owned(),
            ));
        }

        // The sum is of the samples as a WAV file would hold them:
        // interleaved, low byte first.
        let bytes = self.depth.bits() as usize / 8;
        self.md5_bytes.clear();
        for index in 0..count {
            for channel in &self.pending {
                self.md5_bytes
                    .extend_from_slice(&channel[index].to_le_bytes()[..bytes]);
            }
        }
        self.md5.update(&self.md5_bytes);

        let bits = self.depth.bits();
        let limits = RiceLimits {
            max_order: self.effort.rice_order,
            param_bits: if bits > 16 { 5 } else { 4 },
        };
        let work = &mut self.work;
        work.prepare(self.channels, count);
        let assignment = if self.channels == 2 && self.effort.stereo {
            work.plan_stereo(
                &self.pending[0],
                &self.pending[1],
                bits,
                self.effort,
                limits,
            )
        } else {
            for (plan, channel) in work.plans.iter_mut().zip(&self.pending) {
                plan.plan(channel, bits, self.effort, limits, &mut work.bench);
            }
            self.channels as u32 - 1
        };

        let out = &mut self.frame;
        out.clear();
        write_frame_header(out, self.sample_rate, count, assignment, bits, self.frames);
        match assignment {
            LEFT_SIDE => {
                work.plans[0].write(out, &self.pending[0], bits, limits);
                work.plans[3].write(out, &work.side, bits + 1, limits);
            }
            SIDE_RIGHT => {
                work.plans[3].write(out, &work.side, bits + 1, limits);
                work.plans[1].write(out, &self.pending[1], bits, limits);
            }
            MID_SIDE => {
                work.plans[2].write(out, &work.mid, bits, limits);
                work.plans[3].write(out, &work.side, bits + 1, limits);
            }
            _ => {
                for (plan, channel) in work.plans.iter().zip(&self.pending) {
                    plan.write(out, channel, bits, limits);
                }
            }
        }
        out.align();
        let checksum = crc16(out.bytes());
        out.put(u32::from(checksum), 16);

        let frame = out.bytes();
        self.file.write_all(frame)?;
        let (smallest, largest) = self.frame_bytes.unwrap_or((frame.len(), frame.len()));
        self.frame_bytes = Some((smallest.min(frame.len()), largest.max(frame.len())));
        self.frames += 1;
        self.samples += count as u64;
        for channel in &mut self.pending {
            channel.clear();
        }
        Ok(())
    }
}

fn vorbis_comments(tags: &AudioTags<'_>) -> Result<Vec<u8>, CodecError> {
    let fields = [
        ("TITLE", tags.title),
        ("ARTIST", tags.author),
        ("GENRE", tags.genre),
        ("DESCRIPTION", tags.comments),
    ];
    let nonempty = || fields.iter().filter(|(_, value)| !value.is_empty());
    let bytes = nonempty().fold(8 + VENDOR.len() as u64, |total, (key, value)| {
        total + 4 + key.len() as u64 + 1 + value.len() as u64
    });
    // FLAC metadata block lengths have 24 bits.
    if bytes > 0xFF_FFFF {
        return Err(CodecError::InvalidInput(
            "FLAC comments exceed the metadata block limit".to_owned(),
        ));
    }
    let mut comments = Vec::with_capacity(bytes as usize);
    comments.extend_from_slice(&(VENDOR.len() as u32).to_le_bytes());
    comments.extend_from_slice(VENDOR);
    comments.extend_from_slice(&(nonempty().count() as u32).to_le_bytes());
    for (key, value) in nonempty() {
        let size = key.len() + 1 + value.len();
        comments.extend_from_slice(&(size as u32).to_le_bytes());
        comments.extend_from_slice(key.as_bytes());
        comments.push(b'=');
        comments.extend_from_slice(value.as_bytes());
    }
    Ok(comments)
}

fn broken_error() -> CodecError {
    CodecError::InvalidInput("an earlier write to this file failed".to_owned())
}

/// Refuses what a FLAC file cannot hold, in words for the user.
pub(crate) fn check_layout(sample_rate: u32, channels: u16, level: u8) -> Result<(), CodecError> {
    if sample_rate == 0 {
        return Err(CodecError::InvalidInput(
            "the sample rate is zero".to_owned(),
        ));
    }
    if sample_rate > MAX_FLAC_SAMPLE_RATE {
        return Err(CodecError::InvalidInput(
            windfall_core::notice!(
                "codec.flacSampleRate",
                max = MAX_FLAC_SAMPLE_RATE,
                rate = sample_rate
            )
            .to_string(),
        ));
    }
    if channels == 0 {
        return Err(CodecError::InvalidInput("there are no channels".to_owned()));
    }
    if channels > MAX_FLAC_CHANNELS {
        return Err(CodecError::InvalidInput(
            windfall_core::notice!(
                "codec.flacChannels",
                max = MAX_FLAC_CHANNELS,
                channels = channels
            )
            .to_string(),
        ));
    }
    if level > MAX_FLAC_LEVEL {
        return Err(CodecError::InvalidInput(
            windfall_core::notice!("codec.flacLevel", max = MAX_FLAC_LEVEL, level = level)
                .to_string(),
        ));
    }
    Ok(())
}

/// The ways two channels can be stored, as the frame header numbers them.
/// A lower number is that many channels, less one, each stored as it is.
const LEFT_SIDE: u32 = 8;
const SIDE_RIGHT: u32 = 9;
const MID_SIDE: u32 = 10;

fn write_frame_header(
    out: &mut BitWriter,
    sample_rate: u32,
    block: usize,
    assignment: u32,
    bits: u32,
    number: u32,
) {
    // The sync code, and a zero for a stream whose blocks are all one size.
    out.put(0xFFF8, 16);

    let block_code = match block {
        BLOCK => 0b1100,
        1..=256 => 0b0110,
        _ => 0b0111,
    };
    out.put(block_code, 4);
    // Where the rate is not one of the usual ones it follows the header in
    // kHz, in Hz or in tens of Hz, whichever states it exactly. Code zero
    // sends the decoder to the stream's description for it.
    let (rate_code, rate_bits, rate_value) = match sample_rate {
        88_200 => (0b0001, 0, 0),
        176_400 => (0b0010, 0, 0),
        192_000 => (0b0011, 0, 0),
        8_000 => (0b0100, 0, 0),
        16_000 => (0b0101, 0, 0),
        22_050 => (0b0110, 0, 0),
        24_000 => (0b0111, 0, 0),
        32_000 => (0b1000, 0, 0),
        44_100 => (0b1001, 0, 0),
        48_000 => (0b1010, 0, 0),
        96_000 => (0b1011, 0, 0),
        rate if rate % 1_000 == 0 && rate / 1_000 < 256 => (0b1100, 8, rate / 1_000),
        rate if rate < 1 << 16 => (0b1101, 16, rate),
        rate if rate % 10 == 0 && rate / 10 < 1 << 16 => (0b1110, 16, rate / 10),
        _ => (0b0000, 0, 0),
    };
    out.put(rate_code, 4);
    out.put(assignment, 4);
    out.put(if bits == 16 { 0b100 } else { 0b110 }, 3);
    out.put(0, 1);

    // The frame's number, coded the way UTF-8 codes a character.
    match number {
        0..0x80 => out.put(number, 8),
        _ => {
            let tail_bytes = match number {
                0..0x800 => 1,
                0x800..0x1_0000 => 2,
                0x1_0000..0x20_0000 => 3,
                0x20_0000..0x400_0000 => 4,
                _ => 5,
            };
            // As many ones as the number has bytes, then a zero.
            let lead_bits = 6 - tail_bytes;
            out.put(0xFF, tail_bytes + 1);
            out.put(0, 1);
            out.put(number >> (6 * tail_bytes), lead_bits);
            for byte in (0..tail_bytes).rev() {
                out.put(0b10, 2);
                out.put(number >> (6 * byte), 6);
            }
        }
    }
    match block_code {
        0b0110 => out.put(block as u32 - 1, 8),
        0b0111 => out.put(block as u32 - 1, 16),
        _ => {}
    }
    out.put(rate_value, rate_bits);
    let checksum = crc8(out.bytes());
    out.put(u32::from(checksum), 8);
}

/// How one channel of a block is stored.
#[derive(Debug, Clone, Copy, Default)]
enum Kind {
    /// One value for the whole block.
    Constant,
    /// The samples as they are.
    #[default]
    Verbatim,
    /// A fixed predictor of this order.
    Fixed(usize),
    /// A predictor fitted to the block.
    Fitted(Lpc),
}

/// One channel of a block, worked out and ready to write.
#[derive(Debug, Default)]
struct Subframe {
    kind: Kind,
    /// The most bits the subframe takes.
    bits: u64,
    /// What the predictor leaves, for the kinds that have one.
    residual: Vec<i32>,
    rice: RicePlan,
}

/// Memory for working out a subframe, kept from block to block.
#[derive(Debug, Default)]
struct Bench {
    /// A way of storing the channel that is being tried.
    trial: Subframe,
    rice: RiceScratch,
    window: Vec<f64>,
    windowed: Vec<f64>,
}

impl Subframe {
    /// Bits a subframe's first byte and a predictor's first samples take.
    fn head_bits(order: usize, bits: u32) -> u64 {
        8 + order as u64 * u64::from(bits)
    }

    /// Finds the smallest way to store `samples`, which are of `bits` bits.
    fn plan(
        &mut self,
        samples: &[i32],
        bits: u32,
        effort: Effort,
        limits: RiceLimits,
        bench: &mut Bench,
    ) {
        let count = samples.len();
        if samples.iter().all(|&sample| sample == samples[0]) {
            self.kind = Kind::Constant;
            self.bits = 8 + u64::from(bits);
            return;
        }
        self.kind = Kind::Verbatim;
        self.bits = 8 + count as u64 * u64::from(bits);

        let trial = &mut bench.trial;
        for order in 0..=MAX_FIXED_ORDER.min(count - 1) {
            fixed_residual(samples, order, &mut trial.residual);
            trial.kind = Kind::Fixed(order);
            trial.plan_rice(order, limits, &mut bench.rice);
            trial.bits = Self::head_bits(order, bits) + 6 + trial.rice.bits;
            self.keep_smaller(trial);
        }

        // Each coefficient is stored in this many bits. More would only
        // pay with more samples in a block.
        let precision = if bits > 16 { 15 } else { 12 };
        let fit = predict::fit(
            samples,
            effort.lpc_order,
            &bench.window,
            &mut bench.windowed,
        );
        let Some(fit) = fit else {
            return;
        };
        let orders = if effort.every_order {
            1..=fit.orders
        } else {
            let likely = fit.likely_order(count, bits + precision);
            likely..=likely
        };
        for order in orders {
            let Some(lpc) = fit.quantized(order, precision) else {
                continue;
            };
            if !lpc.residual(samples, &mut trial.residual) {
                continue;
            }
            trial.kind = Kind::Fitted(lpc);
            trial.plan_rice(order, limits, &mut bench.rice);
            let coefficient_bits = 4 + 5 + order as u64 * u64::from(precision);
            trial.bits = Self::head_bits(order, bits) + coefficient_bits + 6 + trial.rice.bits;
            self.keep_smaller(trial);
        }
    }

    fn plan_rice(&mut self, order: usize, limits: RiceLimits, scratch: &mut RiceScratch) {
        rice::plan(&self.residual, order, limits, scratch, &mut self.rice);
    }

    /// Takes the place of `trial` if that is smaller. The two trade their
    /// memory, so nothing is copied.
    fn keep_smaller(&mut self, trial: &mut Subframe) {
        if trial.bits < self.bits {
            std::mem::swap(self, trial);
        }
    }

    fn write(&self, out: &mut BitWriter, samples: &[i32], bits: u32, limits: RiceLimits) {
        // A zero, six bits of kind, and a zero for "no bits left out".
        let (kind, order) = match &self.kind {
            Kind::Constant => (0b000000, 0),
            Kind::Verbatim => (0b000001, 0),
            Kind::Fixed(order) => (0b001000 | *order as u32, *order),
            Kind::Fitted(lpc) => (0b100000 | (lpc.order as u32 - 1), lpc.order),
        };
        out.put(kind << 1, 8);
        match &self.kind {
            Kind::Constant => out.put_signed(samples[0], bits),
            Kind::Verbatim => {
                for &sample in samples {
                    out.put_signed(sample, bits);
                }
            }
            Kind::Fixed(_) | Kind::Fitted(_) => {
                for &sample in &samples[..order] {
                    out.put_signed(sample, bits);
                }
                if let Kind::Fitted(lpc) = &self.kind {
                    out.put(lpc.precision - 1, 4);
                    out.put(lpc.shift, 5);
                    for &coefficient in &lpc.coefficients[..order] {
                        out.put_signed(coefficient, lpc.precision);
                    }
                }
                rice::write(out, &self.residual, order, &self.rice, limits);
            }
        }
    }
}

/// Everything the encoder works a block out in, kept from block to block
/// so that no block allocates.
#[derive(Debug, Default)]
struct Workbench {
    /// One subframe per channel. With two channels there are four: left,
    /// right, their mean and their difference.
    plans: Vec<Subframe>,
    mid: Vec<i32>,
    side: Vec<i32>,
    bench: Bench,
}

impl Workbench {
    /// Gets ready for a block of `count` samples on `channels` channels.
    fn prepare(&mut self, channels: usize, count: usize) {
        let plans = if channels == 2 { 4 } else { channels };
        self.plans.resize_with(plans, Subframe::default);
        if self.bench.window.len() != count {
            window(count, &mut self.bench.window);
        }
    }

    /// Works out both channels and the two made from them, and returns the
    /// way of storing the pair that takes the fewest bits.
    fn plan_stereo(
        &mut self,
        left: &[i32],
        right: &[i32],
        bits: u32,
        effort: Effort,
        limits: RiceLimits,
    ) -> u32 {
        self.mid.clear();
        self.side.clear();
        for (&left, &right) in left.iter().zip(right) {
            self.mid.push((left + right) >> 1);
            self.side.push(left - right);
        }
        let bench = &mut self.bench;
        self.plans[0].plan(left, bits, effort, limits, bench);
        self.plans[1].plan(right, bits, effort, limits, bench);
        self.plans[2].plan(&self.mid, bits, effort, limits, bench);
        self.plans[3].plan(&self.side, bits + 1, effort, limits, bench);

        let size = |index: usize| self.plans[index].bits;
        let ways = [
            (1, size(0) + size(1)),
            (LEFT_SIDE, size(0) + size(3)),
            (SIDE_RIGHT, size(3) + size(1)),
            (MID_SIDE, size(2) + size(3)),
        ];
        // The first of the smallest, so plain left and right wins a tie.
        let mut best = ways[0];
        for way in ways {
            if way.1 < best.1 {
                best = way;
            }
        }
        best.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(sample_rate: u32, block: usize, number: u32) -> Vec<u8> {
        let mut out = BitWriter::default();
        write_frame_header(&mut out, sample_rate, block, 1, 16, number);
        out.bytes().to_vec()
    }

    #[test]
    fn a_frame_header_states_rate_and_block_size_in_the_shortest_form() {
        // Sync, block and rate codes, channels and depth, number, checksum.
        let usual = header(44_100, BLOCK, 0);
        assert_eq!(usual[..5], [0xFF, 0xF8, 0xC9, 0x18, 0x00]);
        assert_eq!(usual.len(), 6);
        assert_eq!(usual[5], crc8(&usual[..5]));

        assert_eq!(header(48_000, 100, 0)[2..6], [0x6A, 0x18, 0x00, 99]);
        assert_eq!(header(48_000, 257, 0)[2..7], [0x7A, 0x18, 0x00, 0x01, 0x00]);
        // 50 kHz in kHz, 11,025 Hz in Hz, 352,800 Hz in tens of Hz, and a
        // rate that none of them states left to the stream's description.
        assert_eq!(header(50_000, BLOCK, 0)[2..6], [0xCC, 0x18, 0x00, 50]);
        assert_eq!(
            header(11_025, BLOCK, 0)[2..7],
            [0xCD, 0x18, 0x00, 0x2B, 0x11]
        );
        assert_eq!(
            header(352_800, BLOCK, 0)[2..7],
            [0xCE, 0x18, 0x00, 0x89, 0xD0]
        );
        assert_eq!(header(123_457, BLOCK, 0)[2..5], [0xC0, 0x18, 0x00]);
    }

    #[test]
    fn a_frame_number_is_coded_like_a_character() {
        let number = |number: u32| {
            let bytes = header(44_100, BLOCK, number);
            bytes[4..bytes.len() - 1].to_vec()
        };
        assert_eq!(number(0x7F), [0x7F]);
        assert_eq!(number(0x80), [0xC2, 0x80]);
        assert_eq!(number(0x7FF), [0xDF, 0xBF]);
        assert_eq!(number(0x800), [0xE0, 0xA0, 0x80]);
        assert_eq!(number(0xFFFF), [0xEF, 0xBF, 0xBF]);
        assert_eq!(number(0x1_0000), [0xF0, 0x90, 0x80, 0x80]);
        assert_eq!(number(0x20_0000), [0xF8, 0x88, 0x80, 0x80, 0x80]);
        assert_eq!(number(0x400_0000), [0xFC, 0x84, 0x80, 0x80, 0x80, 0x80]);
        assert_eq!(number(0x7FFF_FFFF), [0xFD, 0xBF, 0xBF, 0xBF, 0xBF, 0xBF]);
    }

    #[test]
    fn a_higher_level_asks_for_no_less() {
        let mut before = Effort::of(0);
        for level in 1..=MAX_FLAC_LEVEL {
            let effort = Effort::of(level);
            // Level 3 is the one step down: it is the first to fit
            // predictors, and leaves the channels as they are to pay for it.
            if level != 3 {
                assert!(effort.lpc_order >= before.lpc_order, "{level}");
                assert!(effort.stereo >= before.stereo, "{level}");
            }
            before = effort;
        }
        assert!(Effort::of(MAX_FLAC_LEVEL).every_order);
        assert!(!Effort::of(DEFAULT_FLAC_LEVEL).every_order);
    }
}
