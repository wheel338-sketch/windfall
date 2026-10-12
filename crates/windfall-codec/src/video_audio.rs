//! Control-side video audio demux. Reuses the ordinary bounded audio decoder;
//! no codec runs on the callback. AVI PCM and Ogg/Vorbis are supported. Other
//! containers/codecs return the existing explicit decoder-unavailable error.
use crate::{CodecError, DecodeOptions, decode_bytes_with};
use windfall_core::AudioBuffer;

pub const MAX_VIDEO_FILE_BYTES: u64 = 64 * 1024 * 1024;

pub fn decode_video_audio_file(path: &std::path::Path) -> Result<AudioBuffer, CodecError> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > MAX_VIDEO_FILE_BYTES {
        return Err(CodecError::TooLarge {
            limit_bytes: MAX_VIDEO_FILE_BYTES,
        });
    }
    let mut bytes = Vec::new();
    file.take(MAX_VIDEO_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_VIDEO_FILE_BYTES {
        return Err(CodecError::TooLarge {
            limit_bytes: MAX_VIDEO_FILE_BYTES,
        });
    }
    decode_video_audio(
        &bytes,
        &DecodeOptions {
            max_decoded_bytes: MAX_VIDEO_FILE_BYTES,
        },
    )
}

pub fn decode_video_audio(
    bytes: &[u8],
    options: &DecodeOptions,
) -> Result<AudioBuffer, CodecError> {
    if bytes.len() as u64 > MAX_VIDEO_FILE_BYTES {
        return Err(CodecError::TooLarge {
            limit_bytes: MAX_VIDEO_FILE_BYTES,
        });
    }
    if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"AVI ") {
        return decode_bytes_with(bytes, None, options);
    }
    let corrupt = || CodecError::Corrupt("invalid AVI audio chunks".into());
    let declared = word(bytes, 4)? as usize + 8;
    if declared > bytes.len() || declared < 12 {
        return Err(corrupt());
    }
    let mut format = None;
    let mut stream = None;
    let mut stream_number = 0_u8;
    let mut start_frames = 0_u32;
    let mut scale = 0_u32;
    let mut rate = 0_u32;
    let mut data = Vec::new();
    // Bounded depth rejects hostile nested LISTs without recursive stack growth.
    type ChunkVisitor<'a, 'callback> =
        dyn FnMut(&[u8], &'a [u8]) -> Result<(), CodecError> + 'callback;
    fn walk<'a>(
        bytes: &'a [u8],
        depth: u8,
        visit: &mut ChunkVisitor<'a, '_>,
    ) -> Result<(), CodecError> {
        if depth > 8 {
            return Err(CodecError::Corrupt(
                "AVI nesting exceeds eight lists".into(),
            ));
        }
        let mut at = 0;
        while at < bytes.len() {
            let id = bytes
                .get(at..at + 4)
                .ok_or_else(|| CodecError::Corrupt("truncated AVI chunk".into()))?;
            let size = word(bytes, at + 4)? as usize;
            let begin = at + 8;
            let end = begin
                .checked_add(size)
                .filter(|end| *end <= bytes.len())
                .ok_or_else(|| CodecError::Corrupt("truncated AVI payload".into()))?;
            let payload = &bytes[begin..end];
            visit(id, payload)?;
            if id == b"LIST" {
                walk(
                    payload
                        .get(4..)
                        .ok_or_else(|| CodecError::Corrupt("empty AVI LIST".into()))?,
                    depth + 1,
                    visit,
                )?;
            }
            at = end + (size & 1);
            if at > bytes.len() {
                return Err(CodecError::Corrupt("missing AVI padding".into()));
            }
        }
        Ok(())
    }
    walk(&bytes[12..declared], 0, &mut |id, payload| {
        if id == b"strh" {
            if payload.len() < 56 {
                return Err(corrupt());
            }
            if stream.is_none() && &payload[..4] == b"auds" {
                stream = Some(stream_number);
                // AVI audio dwStart is in stream samples. PCM's stream scale
                // must describe exactly one block-aligned sample frame.
                start_frames = word(payload, 28)?;
                scale = word(payload, 20)?;
                rate = word(payload, 24)?;
            }
            stream_number = stream_number.checked_add(1).ok_or_else(corrupt)?;
        } else if id == b"strf" && stream == stream_number.checked_sub(1) && format.is_none() {
            if payload.len() < 16 || payload[..2] != [1, 0] {
                return Err(CodecError::UnsupportedFormat(
                    "AVI audio decoder missing (PCM required)".into(),
                ));
            }
            format = Some(payload[..16].to_vec());
        } else if id.len() == 4 && &id[2..] == b"wb" {
            let number = (id[0].wrapping_sub(b'0'))
                .checked_mul(10)
                .and_then(|n| n.checked_add(id[1].wrapping_sub(b'0')));
            if number == stream {
                if data.len().saturating_add(payload.len()) as u64 > options.max_decoded_bytes {
                    return Err(CodecError::TooLarge {
                        limit_bytes: options.max_decoded_bytes,
                    });
                }
                data.extend_from_slice(payload);
            }
        }
        Ok(())
    })?;
    let format = format.ok_or(CodecError::NoAudio)?;
    let align = u16::from_le_bytes([format[12], format[13]]) as usize;
    if align == 0 || !data.len().is_multiple_of(align) {
        return Err(corrupt());
    }
    let sample_rate = word(&format, 4)?;
    if rate == 0 || scale == 0 {
        return Err(corrupt());
    }
    let start = u64::from(start_frames)
        .checked_mul(u64::from(scale))
        .and_then(|n| n.checked_mul(u64::from(sample_rate)))
        .ok_or_else(corrupt)?;
    if !start.is_multiple_of(u64::from(rate)) {
        return Err(CodecError::UnsupportedFormat(
            "fractional AVI audio start is unsupported".into(),
        ));
    }
    let padding = usize::try_from(start / u64::from(rate))
        .ok()
        .and_then(|n| n.checked_mul(align))
        .ok_or_else(corrupt)?;
    if padding.saturating_add(data.len()) as u64 > options.max_decoded_bytes {
        return Err(CodecError::TooLarge {
            limit_bytes: options.max_decoded_bytes,
        });
    }
    let length = padding + data.len();
    let mut wav = Vec::with_capacity(44 + length + (length & 1));
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + length as u32 + (length as u32 & 1)).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&format);
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(length as u32).to_le_bytes());
    wav.resize(44 + padding, if format[14] == 8 { 128 } else { 0 });
    wav.extend_from_slice(&data);
    if length & 1 != 0 {
        wav.push(0);
    }
    decode_bytes_with(&wav, Some("wav"), options)
}

fn word(bytes: &[u8], at: usize) -> Result<u32, CodecError> {
    let value = bytes
        .get(at..at + 4)
        .ok_or_else(|| CodecError::Corrupt("truncated AVI header".into()))?;
    Ok(u32::from_le_bytes(
        value.try_into().expect("four-byte slice"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn chunk(id: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        if data.len() & 1 != 0 {
            out.push(0);
        }
        out
    }
    #[test]
    fn avi_pcm_demux_preserves_stream_start_and_interleaved_samples() {
        let mut header = [0_u8; 56];
        header[..4].copy_from_slice(b"auds");
        header[20..24].copy_from_slice(&4_u32.to_le_bytes());
        header[24..28].copy_from_slice(&192000_u32.to_le_bytes());
        header[28..32].copy_from_slice(&2_u32.to_le_bytes());
        let format: [u8; 16] = [1, 0, 2, 0, 0x80, 0xbb, 0, 0, 0, 0xee, 2, 0, 4, 0, 16, 0];
        let samples = [32767_i16, -32768, 16384, -16384];
        let pcm: Vec<_> = samples.iter().flat_map(|n| n.to_le_bytes()).collect();
        let mut body = b"AVI ".to_vec();
        body.extend(chunk(b"strh", &header));
        body.extend(chunk(b"strf", &format));
        body.extend(chunk(b"00wb", &pcm));
        let audio = decode_video_audio(&chunk(b"RIFF", &body), &DecodeOptions::default()).unwrap();
        assert_eq!(audio.sample_rate(), 48000);
        assert_eq!(audio.channels(), 2);
        assert_eq!(
            audio.samples(),
            &[0.0, 0.0, 0.0, 0.0, 32767.0 / 32768.0, -1.0, 0.5, -0.5]
        );
    }
    #[test]
    fn missing_video_decoder_and_truncated_avi_are_errors() {
        assert!(decode_video_audio(b"unsupported mp4", &DecodeOptions::default()).is_err());
        let mut bytes = b"RIFF\xff\xff\xff\xffAVI ".to_vec();
        assert!(decode_video_audio(&bytes, &DecodeOptions::default()).is_err());
        bytes[4..8].copy_from_slice(&4_u32.to_le_bytes());
        assert!(decode_video_audio(&bytes, &DecodeOptions::default()).is_err());
    }
}
