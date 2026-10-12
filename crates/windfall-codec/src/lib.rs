//! Audio files in and out of Windfall.
//!
//! Reading: WAV, AIFF, FLAC, MP3 and Ogg Vorbis are decoded whole into an
//! [`AudioBuffer`](windfall_core::AudioBuffer) at the file's own sample rate
//! and channel count. Nothing is resampled here; the engine does that at
//! playback.
//!
//! Writing: WAV, FLAC, Ogg Vorbis and MP3, a block at a time.
//! [`Encoder`] writes any of them the same way.
//!
//! Drawing: [`peaks`] reduces a buffer to a waveform overview.

mod atomic;
mod decode;
mod encoder;
mod error;
mod flac;
mod guard;
mod md5;
mod mp3;
mod peaks;
mod vorbis;
mod wav;
mod wav_metadata;
pub use wav_metadata::{
    MAX_WAV_MARKERS, MAX_WAV_METADATA_BYTES, WavCue, WavLoop, WavLoopKind, WavMetadata, WavSlice,
    WavTempo, read_wav_metadata, read_wav_metadata_file,
};

pub use decode::{
    AudioInfo, DEFAULT_MAX_DECODED_BYTES, DecodeOptions, decode_bytes, decode_bytes_strict_with,
    decode_bytes_with, decode_file, decode_file_strict_with, decode_file_with, probe_bytes,
    probe_file,
};
pub use encoder::{AudioFormat, AudioTags, Encoder, EncoderSettings};
pub use error::CodecError;
pub use flac::{
    DEFAULT_FLAC_LEVEL, FlacBitDepth, FlacWriter, MAX_FLAC_CHANNELS, MAX_FLAC_LEVEL,
    MAX_FLAC_SAMPLE_RATE, write_flac,
};
pub use mp3::{MP3_SAMPLE_RATES, Mp3Channels, Mp3Rate, Mp3Settings, Mp3Writer};
pub use peaks::peaks;
pub use vorbis::{
    DEFAULT_VORBIS_QUALITY, MAX_VORBIS_QUALITY, MIN_VORBIS_QUALITY, VORBIS_SAMPLE_RATES,
    VorbisWriter, write_vorbis,
};
pub use wav::{DEFAULT_DITHER_SEED, WavSampleFormat, WavWriter, write_wav};

/// File extensions Windfall can decode, lowercase and without the dot.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "wav", "wave", "aif", "aiff", "aifc", "flac", "mp3", "ogg", "oga", "ds",
];

/// Whether a file extension (without the dot, in any letter case) names a
/// format Windfall can decode.
pub fn is_audio_extension(ext: &str) -> bool {
    AUDIO_EXTENSIONS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_extensions_match_in_any_case() {
        for ext in ["wav", "WAV", "Flac", "mp3", "ogg", "aiff", "AIF"] {
            assert!(is_audio_extension(ext), "{ext}");
        }
    }

    #[test]
    fn other_extensions_do_not_match() {
        for ext in ["", ".wav", "txt", "mid", "windfall", "wav ", "mp4"] {
            assert!(!is_audio_extension(ext), "{ext:?}");
        }
    }

    #[test]
    fn the_extension_list_is_lowercase_without_dots() {
        for ext in AUDIO_EXTENSIONS {
            assert!(!ext.is_empty());
            assert!(
                ext.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            );
        }
    }
}

mod m4a;
pub use m4a::{
    AacConfig, AacEncoderAdapter, AacPacket, AacTrim, M4aWriter, MAX_AAC_PACKET_BYTES,
    MAX_M4A_PACKETS,
};

mod drumsynth;
pub use drumsynth::{
    DRUMSYNTH_SOURCE_RATE, DRUMSYNTH_SOURCE_SEED, DrumSynthPreset, DrumSynthRendered,
    MAX_DRUMSYNTH_FRAMES, MAX_DRUMSYNTH_PRESET_BYTES, render_drumsynth_file,
    render_drumsynth_source,
};
