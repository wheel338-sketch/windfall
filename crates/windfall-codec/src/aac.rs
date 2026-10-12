//! OS-owned AAC encoding. The application ships no AAC codec implementation.
#[cfg(not(windows))]
use crate::AudioTags;
use crate::{AacConfig, CodecError};
#[cfg(not(windows))]
use std::path::Path;

pub const AAC_SAMPLE_RATES: &[u32] = &[44100, 48000];
pub const AAC_BITRATES: &[u16] = &[96, 128, 160, 192];
pub(crate) fn check_layout(rate: u32, channels: u16, bitrate: u16) -> Result<(), CodecError> {
    if !AAC_SAMPLE_RATES.contains(&rate)
        || ![1, 2].contains(&channels)
        || !AAC_BITRATES.contains(&bitrate)
    {
        return Err(CodecError::InvalidInput(
            "M4A AAC requires 44.1/48 kHz, mono/stereo, and 96/128/160/192 kbps".into(),
        ));
    }
    #[cfg(not(windows))]
    {
        Err(unavailable())
    }
    #[cfg(windows)]
    {
        Ok(())
    }
}
#[cfg(not(windows))]
fn unavailable() -> CodecError {
    CodecError::UnsupportedFormat("M4A encoding requires Windows Media Foundation's AAC encoder (install the Media Feature Pack on Windows N)".into())
}
/// Checks actual encoder + decoder activation, type negotiation and alignment
/// without opening a destination. A configuration without these OS components
/// cannot provide verified gapless export and is reported unavailable.
pub fn m4a_capability() -> Result<(), CodecError> {
    #[cfg(windows)]
    {
        mf::Encoder::create(
            AacConfig {
                sample_rate: 48000,
                channels: 2,
            },
            192,
        )
        .map(drop)
    }
    #[cfg(not(windows))]
    {
        Err(unavailable())
    }
}

#[cfg(windows)]
mod mf;
#[cfg(windows)]
pub use mf::NativeAacWriter;

#[cfg(not(windows))]
#[derive(Debug)]
pub struct NativeAacWriter;
#[cfg(not(windows))]
impl NativeAacWriter {
    pub fn create(
        _path: impl AsRef<Path>,
        _config: AacConfig,
        _bitrate: u16,
        _tags: &AudioTags<'_>,
    ) -> Result<Self, CodecError> {
        Err(unavailable())
    }
    pub fn write(&mut self, _pcm: &[f32]) -> Result<(), CodecError> {
        Err(unavailable())
    }
    pub fn finalize(self) -> Result<(), CodecError> {
        Err(unavailable())
    }
}

/// Decode our bounded AAC-LC MP4 layout through the OS decoder, honoring its
/// edit list. Useful for native sample import and independent export acceptance.
pub fn decode_m4a_bytes(
    bytes: &[u8],
    options: &crate::DecodeOptions,
) -> Result<windfall_core::AudioBuffer, CodecError> {
    #[cfg(windows)]
    {
        mf::decode_container(bytes, options)
    }
    #[cfg(not(windows))]
    {
        let _ = (bytes, options);
        Err(unavailable())
    }
}
