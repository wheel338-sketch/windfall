//! Writing Ogg Vorbis: lossy, a tenth of the size of a WAV file or less.
//!
//! The encoder is the reference one, libvorbis with the aoTuV tuning, bound
//! by the `vorbis_rs` crate and built from the C sources that crate
//! carries.

use std::num::{NonZeroU8, NonZeroU32};
use std::path::Path;

use vorbis_rs::{
    VorbisBitrateManagementStrategy, VorbisEncoder, VorbisEncoderBuilder, VorbisError,
};
use windfall_core::AudioBuffer;

use crate::atomic::AtomicFile;
use crate::encoder::AudioTags;
use crate::error::CodecError;

/// The lowest quality an Ogg Vorbis file can be asked for: about 45 kbit/s
/// for stereo at 44.1 kHz.
pub const MIN_VORBIS_QUALITY: f32 = -1.0;

/// The highest quality: about 500 kbit/s for stereo at 44.1 kHz.
pub const MAX_VORBIS_QUALITY: f32 = 10.0;

/// The quality an Ogg Vorbis file gets unless another is asked for: about
/// 190 kbit/s for stereo at 44.1 kHz, which few people can tell from the
/// original.
pub const DEFAULT_VORBIS_QUALITY: f32 = 6.0;

/// The sample rates the encoder has settings for.
pub const VORBIS_SAMPLE_RATES: std::ops::RangeInclusive<u32> = 8_000..=192_000;

/// The number every Ogg page of a file carries to tell its stream from
/// others. Encoders usually draw it at random; a fixed one makes the same
/// audio give the same file.
const STREAM_SERIAL: i32 = 0x5746_4C4C;

/// Frames handed to the encoder at a time.
const FRAMES_PER_CHUNK: usize = 4096;

/// Writes a whole buffer to an Ogg Vorbis file. See [`VorbisWriter`].
pub fn write_vorbis(
    path: impl AsRef<Path>,
    buffer: &AudioBuffer,
    quality: f32,
) -> Result<(), CodecError> {
    let (rate, channels) = (buffer.sample_rate(), buffer.channels());
    let mut writer = VorbisWriter::create(path, rate, channels, quality)?;
    writer.write(buffer.samples())?;
    writer.finalize()
}

/// Writes an Ogg Vorbis file a block at a time, for exports too long to
/// hold in memory.
///
/// The quality runs from [`MIN_VORBIS_QUALITY`] to [`MAX_VORBIS_QUALITY`],
/// the scale the reference encoder's `-q` uses. The bit rate follows the
/// music: a quality is a promise about how it sounds, not about its size.
/// The file decodes to exactly as many frames as were written, and the same
/// input always gives the same file.
///
/// The audio goes to a temporary file beside the destination and takes the
/// destination's name only in [`finalize`](Self::finalize). If the writer is
/// dropped first, or anything fails, the temporary file is removed and
/// whatever was at the destination before is left untouched.
pub struct VorbisWriter {
    /// `None` once the stream has been ended in `finalize`.
    encoder: Option<VorbisEncoder<AtomicFile>>,
    /// The block being handed over, one list of samples for each channel.
    planar: Vec<Vec<f32>>,
    /// Set while a write is under way and left set if it fails, because the
    /// file then holds part of a block.
    broken: bool,
}

impl std::fmt::Debug for VorbisWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VorbisWriter")
            .field("channels", &self.planar.len())
            .field("broken", &self.broken)
            .finish_non_exhaustive()
    }
}

impl VorbisWriter {
    /// Starts an Ogg Vorbis file that will appear at `path` when finalized.
    pub fn create(
        path: impl AsRef<Path>,
        sample_rate: u32,
        channels: u16,
        quality: f32,
    ) -> Result<Self, CodecError> {
        Self::create_with_tags(path, sample_rate, channels, quality, &AudioTags::default())
    }

    /// Starts an Ogg Vorbis file with comments for nonempty project fields.
    pub fn create_with_tags(
        path: impl AsRef<Path>,
        sample_rate: u32,
        channels: u16,
        quality: f32,
        tags: &AudioTags<'_>,
    ) -> Result<Self, CodecError> {
        let (rate, count) = check_layout(sample_rate, channels, quality)?;
        let file = AtomicFile::create(path.as_ref())?;
        let mut builder = VorbisEncoderBuilder::new_with_serial(rate, count, file, STREAM_SERIAL);
        builder.bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr {
            target_quality: quality / 10.0,
        });
        for (tag, value) in [
            ("TITLE", tags.title),
            ("ARTIST", tags.author),
            ("GENRE", tags.genre),
            ("DESCRIPTION", tags.comments),
        ] {
            if !value.is_empty() {
                builder.comment_tag(tag, value).map_err(from_vorbis)?;
            }
        }
        let encoder = builder.build().map_err(from_vorbis)?;
        Ok(Self {
            encoder: Some(encoder),
            planar: (0..channels)
                .map(|_| Vec::with_capacity(FRAMES_PER_CHUNK))
                .collect(),
            broken: false,
        })
    }

    /// Appends interleaved samples. The length must be a whole number of
    /// frames.
    pub fn write(&mut self, interleaved: &[f32]) -> Result<(), CodecError> {
        if self.broken {
            return Err(broken_error());
        }
        let channels = self.planar.len();
        let encoder = self.encoder.as_mut().ok_or_else(broken_error)?;
        if !interleaved.len().is_multiple_of(channels) {
            return Err(CodecError::InvalidInput(
                windfall_core::notice!(
                    "codec.partialFrames",
                    samples = interleaved.len(),
                    channels = channels
                )
                .to_string(),
            ));
        }
        self.broken = true;
        let mut remaining = interleaved;
        while !remaining.is_empty() {
            let frames = (FRAMES_PER_CHUNK - self.planar[0].len()).min(remaining.len() / channels);
            let (chunk, rest) = remaining.split_at(frames * channels);
            remaining = rest;
            for (index, channel) in self.planar.iter_mut().enumerate() {
                let samples = chunk.iter().skip(index).step_by(channels);
                // The encoder's model of hearing has no place for what is
                // not a number.
                channel
                    .extend(samples.map(|&sample| if sample.is_finite() { sample } else { 0.0 }));
            }
            if self.planar[0].len() == FRAMES_PER_CHUNK {
                encoder
                    .encode_audio_block(&self.planar)
                    .map_err(from_vorbis)?;
                for channel in &mut self.planar {
                    channel.clear();
                }
            }
        }
        self.broken = false;
        Ok(())
    }

    /// Ends the stream and moves the file to its destination, replacing any
    /// file already there.
    pub fn finalize(mut self) -> Result<(), CodecError> {
        if self.broken {
            return Err(broken_error());
        }
        let mut encoder = self.encoder.take().ok_or_else(broken_error)?;
        if !self.planar[0].is_empty() {
            encoder
                .encode_audio_block(&self.planar)
                .map_err(from_vorbis)?;
        }
        let file = encoder.finish().map_err(from_vorbis)?;
        file.place(|_| Ok(()))?;
        Ok(())
    }
}

fn broken_error() -> CodecError {
    CodecError::InvalidInput("an earlier write to this file failed".to_owned())
}

fn from_vorbis(error: VorbisError) -> CodecError {
    match error {
        VorbisError::Io(error) => CodecError::Io(error),
        other => CodecError::Encoding(other.to_string()),
    }
}

/// Refuses what this encoder cannot write, in words for the user, and
/// gives back the rate and the channel count as the encoder takes them.
pub(crate) fn check_layout(
    sample_rate: u32,
    channels: u16,
    quality: f32,
) -> Result<(NonZeroU32, NonZeroU8), CodecError> {
    let rate = NonZeroU32::new(sample_rate)
        .filter(|_| VORBIS_SAMPLE_RATES.contains(&sample_rate))
        .ok_or_else(|| {
            CodecError::InvalidInput(
                windfall_core::notice!(
                    "codec.vorbisSampleRate",
                    min = VORBIS_SAMPLE_RATES.start(),
                    max = VORBIS_SAMPLE_RATES.end(),
                    rate = sample_rate
                )
                .to_string(),
            )
        })?;
    // More channels are possible in the format, in an order of their own
    // that nothing here maps to.
    let count = u8::try_from(channels)
        .ok()
        .and_then(NonZeroU8::new)
        .filter(|count| count.get() <= 2)
        .ok_or_else(|| {
            CodecError::InvalidInput(
                windfall_core::notice!("codec.vorbisChannels", channels = channels).to_string(),
            )
        })?;
    if !(MIN_VORBIS_QUALITY..=MAX_VORBIS_QUALITY).contains(&quality) {
        return Err(CodecError::InvalidInput(
            windfall_core::notice!(
                "codec.vorbisQuality",
                min = MIN_VORBIS_QUALITY,
                max = MAX_VORBIS_QUALITY,
                quality = quality
            )
            .to_string(),
        ));
    }
    Ok((rate, count))
}
