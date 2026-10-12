//! One way to write every format Windfall exports.

use std::path::Path;

use crate::error::CodecError;
use crate::flac::{self, FlacBitDepth, FlacWriter};
use crate::mp3::{self, Mp3Settings, Mp3Writer};
use crate::vorbis::{self, VorbisWriter};
use crate::wav::{self, WavSampleFormat, WavWriter};

/// Project information carried by audio exports. Empty fields are omitted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AudioTags<'a> {
    pub title: &'a str,
    pub author: &'a str,
    pub genre: &'a str,
    pub comments: &'a str,
}

/// The audio file formats Windfall writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AudioFormat {
    Wav,
    Flac,
    /// Vorbis audio in an Ogg file.
    Ogg,
    /// MPEG Layer III audio with gapless delay and padding metadata.
    Mp3,
    WavPack,
    M4a,
}

impl AudioFormat {
    /// The ending of a file of this format, lowercase and without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::Ogg => "ogg",
            Self::Mp3 => "mp3",
            Self::WavPack => "wv",
            Self::M4a => "m4a",
        }
    }

    /// The name of the format, as a user knows it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Wav => "WAV",
            Self::Flac => "FLAC",
            Self::Ogg => "Ogg Vorbis",
            Self::Mp3 => "MP3",
            Self::WavPack => "WavPack",
            Self::M4a => "M4A (AAC)",
        }
    }
}

/// A format and how to write it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EncoderSettings {
    /// Uncompressed. See [`WavWriter`].
    Wav { format: WavSampleFormat },
    /// Lossless. `level` is the compression level, 0 to
    /// [`MAX_FLAC_LEVEL`](crate::MAX_FLAC_LEVEL). See [`FlacWriter`].
    Flac { depth: FlacBitDepth, level: u8 },
    /// Lossy. `quality` runs from
    /// [`MIN_VORBIS_QUALITY`](crate::MIN_VORBIS_QUALITY) to
    /// [`MAX_VORBIS_QUALITY`](crate::MAX_VORBIS_QUALITY). One or two
    /// channels. See [`VorbisWriter`].
    Vorbis { quality: f32 },
    /// Lossy MP3 rate and channel coding. See [`Mp3Writer`].
    Mp3 { settings: Mp3Settings },
    WavPack { depth: crate::WavPackDepth },
    M4a { bitrate: u16 },
}

impl EncoderSettings {
    pub fn format(&self) -> AudioFormat {
        match self {
            Self::Wav { .. } => AudioFormat::Wav,
            Self::Flac { .. } => AudioFormat::Flac,
            Self::Vorbis { .. } => AudioFormat::Ogg,
            Self::Mp3 { .. } => AudioFormat::Mp3,
            Self::WavPack { .. } => AudioFormat::WavPack,
            Self::M4a { .. } => AudioFormat::M4a,
        }
    }

    /// Checks format/layout settings without creating a file. M4A activation
    /// additionally depends on the installed OS codec; [`crate::m4a_capability`]
    /// checks that capability and `open` propagates any activation error.
    pub fn check(&self, sample_rate: u32, channels: u16) -> Result<(), CodecError> {
        match *self {
            Self::WavPack { .. } => crate::wavpack::check_layout(sample_rate, channels),
            Self::M4a { bitrate } => crate::aac::check_layout(sample_rate, channels, bitrate),
            Self::Mp3 { settings } => mp3::check_layout(sample_rate, channels, settings),
            Self::Wav { format } => wav::check_layout(sample_rate, channels, format),
            Self::Flac { level, .. } => flac::check_layout(sample_rate, channels, level),
            Self::Vorbis { quality } => {
                vorbis::check_layout(sample_rate, channels, quality).map(drop)
            }
        }
    }
}

/// Writes an audio file of any format Windfall exports, a block at a time.
///
/// Every format is written the same way: [`open`](Self::open),
/// [`write`](Self::write) interleaved samples as often as needed, then
/// [`finalize`](Self::finalize). The audio goes to a temporary file beside
/// the destination, which takes the destination's name only in `finalize`.
/// An encoder that is dropped first removes the temporary file and leaves
/// whatever was at the destination untouched.
///
/// The same samples and settings always give the same file, byte for byte:
/// nothing in a file comes from the clock or from the system's randomness.
/// M4A output also depends on the installed operating system codec version.
#[derive(Debug)]
pub struct Encoder {
    inner: Inner,
}

#[derive(Debug)]
enum Inner {
    Wav(WavWriter),
    Flac(Box<FlacWriter>),
    Vorbis(Box<VorbisWriter>),
    Mp3(Mp3Writer),
    WavPack(crate::WavPackWriter),
    M4a(Box<crate::NativeAacWriter>),
}

impl Encoder {
    /// Starts a file that will appear at `path` when finalized.
    pub fn open(
        path: impl AsRef<Path>,
        settings: &EncoderSettings,
        sample_rate: u32,
        channels: u16,
    ) -> Result<Self, CodecError> {
        Self::open_with_tags(path, settings, sample_rate, channels, &AudioTags::default())
    }

    /// Starts a file with project tags for WAV, FLAC, Ogg Vorbis, or MP3.
    pub fn open_with_tags(
        path: impl AsRef<Path>,
        settings: &EncoderSettings,
        sample_rate: u32,
        channels: u16,
        tags: &AudioTags<'_>,
    ) -> Result<Self, CodecError> {
        Self::open_with_wav_metadata(
            path,
            settings,
            sample_rate,
            channels,
            tags,
            &crate::WavMetadata::default(),
        )
    }

    /// WAV writes the supplied metadata. Other formats reject nonempty WAV
    /// metadata instead of reporting successful preservation.
    pub fn open_with_wav_metadata(
        path: impl AsRef<Path>,
        settings: &EncoderSettings,
        sample_rate: u32,
        channels: u16,
        tags: &AudioTags<'_>,
        metadata: &crate::WavMetadata,
    ) -> Result<Self, CodecError> {
        if !matches!(settings, EncoderSettings::Wav { .. })
            && *metadata != crate::WavMetadata::default()
        {
            return Err(CodecError::InvalidInput(
                "WAV markers require WAV export".into(),
            ));
        }
        let inner = match *settings {
            EncoderSettings::WavPack { depth } => Inner::WavPack(crate::WavPackWriter::create_with_tags(
                path, sample_rate, channels, depth, crate::DEFAULT_WAVPACK_BLOCK_FRAMES, tags,
            )?),
            EncoderSettings::M4a { bitrate } => Inner::M4a(Box::new(crate::NativeAacWriter::create(
                path, crate::AacConfig { sample_rate, channels }, bitrate, tags,
            )?)),
            EncoderSettings::Mp3 { settings } => Inner::Mp3(Mp3Writer::create_with_tags(
                path,
                sample_rate,
                channels,
                settings,
                tags,
            )?),
            EncoderSettings::Wav { format } => Inner::Wav(WavWriter::create_with_metadata(
                path,
                sample_rate,
                channels,
                format,
                tags,
                metadata,
            )?),
            EncoderSettings::Flac { depth, level } => {
                let writer =
                    FlacWriter::create_with_tags(path, sample_rate, channels, depth, level, tags)?;
                Inner::Flac(Box::new(writer))
            }
            EncoderSettings::Vorbis { quality } => {
                let writer =
                    VorbisWriter::create_with_tags(path, sample_rate, channels, quality, tags)?;
                Inner::Vorbis(Box::new(writer))
            }
        };
        Ok(Self { inner })
    }

    /// Appends interleaved samples. The length must be a whole number of
    /// frames.
    pub fn write(&mut self, interleaved: &[f32]) -> Result<(), CodecError> {
        match &mut self.inner {
            Inner::Wav(writer) => writer.write(interleaved),
            Inner::Flac(writer) => writer.write(interleaved),
            Inner::Vorbis(writer) => writer.write(interleaved),
            Inner::Mp3(writer) => writer.write(interleaved),
            Inner::WavPack(writer) => writer.write(interleaved),
            Inner::M4a(writer) => writer.write(interleaved),
        }
    }

    /// Completes the file and moves it to its destination, replacing any
    /// file already there.
    pub fn finalize(self) -> Result<(), CodecError> {
        match self.inner {
            Inner::Wav(writer) => writer.finalize(),
            Inner::Flac(writer) => writer.finalize(),
            Inner::Vorbis(writer) => writer.finalize(),
            Inner::Mp3(writer) => writer.finalize(),
            Inner::WavPack(writer) => writer.finalize(),
            Inner::M4a(writer) => writer.finalize(),
        }
    }
}
