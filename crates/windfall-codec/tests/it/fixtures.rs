//! Decodes the ffmpeg-made files in `tests/fixtures` and checks them against
//! the formula they were generated from.

use std::fs;

use windfall_codec::{
    CodecError, DecodeOptions, decode_bytes, decode_file, decode_file_with, probe_bytes, probe_file,
};

use crate::common::*;

/// What `generate.sh` put into one fixture.
struct Fixture {
    file: &'static str,
    sample_rate: u32,
    /// The tone on each channel, in channel order.
    frequencies: &'static [f64],
    frames: usize,
    container: &'static str,
    codec: &'static str,
    bits: Option<u32>,
    /// Largest allowed distance of any decoded sample from the formula.
    tolerance: f32,
}

const STEREO: &[f64] = &[440.0, 1_000.0];
const MONO: &[f64] = &[440.0];
const SURROUND: &[f64] = &[300.0, 400.0, 500.0, 600.0, 700.0, 800.0];

/// One quantization step at each integer depth. ffmpeg converts without
/// dither, so lossless files stay within a step of the formula.
const STEP_8: f32 = 1.0 / 128.0;
const STEP_16: f32 = 1.0 / 32_768.0;
/// ffmpeg cuts 24-bit samples off instead of rounding them, which uses the
/// whole step, so `f32` rounding needs room on top.
const STEP_24: f32 = 1.5 / 8_388_608.0;
/// 32-bit integers and floats are finer than the `f32` they are decoded to,
/// so only its rounding is left.
const FLOAT_ROUNDING: f32 = 1.0e-7;
/// The lossy files measure 0.01 to 0.035 from the formula. Encoder delay left
/// in would move the tone by hundreds of frames and put it near 1.0.
const LOSSY: f32 = 0.05;

#[rustfmt::skip]
const FIXTURES: &[Fixture] = &[
    Fixture { file: "wav_u8_22k_mono.wav", sample_rate: 22_050, frequencies: MONO, frames: 2_205, container: "wave", codec: "pcm_u8", bits: Some(8), tolerance: STEP_8 },
    Fixture { file: "wav_s16_44k_stereo.wav", sample_rate: 44_100, frequencies: STEREO, frames: 4_410, container: "wave", codec: "pcm_s16le", bits: Some(16), tolerance: STEP_16 },
    Fixture { file: "wav_s24_48k_stereo.wav", sample_rate: 48_000, frequencies: STEREO, frames: 4_800, container: "wave", codec: "pcm_s24le", bits: Some(24), tolerance: STEP_24 },
    Fixture { file: "wav_s32_44k_stereo.wav", sample_rate: 44_100, frequencies: STEREO, frames: 4_410, container: "wave", codec: "pcm_s32le", bits: Some(32), tolerance: FLOAT_ROUNDING },
    Fixture { file: "wav_f32_48k_stereo.wav", sample_rate: 48_000, frequencies: STEREO, frames: 4_800, container: "wave", codec: "pcm_f32le", bits: Some(32), tolerance: FLOAT_ROUNDING },
    Fixture { file: "wav_f64_44k_mono.wav", sample_rate: 44_100, frequencies: MONO, frames: 4_410, container: "wave", codec: "pcm_f64le", bits: Some(64), tolerance: FLOAT_ROUNDING },
    Fixture { file: "wav_s16_48k_surround.wav", sample_rate: 48_000, frequencies: SURROUND, frames: 4_800, container: "wave", codec: "pcm_s16le", bits: Some(16), tolerance: STEP_16 },
    Fixture { file: "wav_with_list_chunk.wav", sample_rate: 44_100, frequencies: STEREO, frames: 4_410, container: "wave", codec: "pcm_s16le", bits: Some(16), tolerance: STEP_16 },
    Fixture { file: "wav_unknown_length.wav", sample_rate: 44_100, frequencies: STEREO, frames: 4_410, container: "wave", codec: "pcm_s16le", bits: Some(16), tolerance: STEP_16 },
    Fixture { file: "aiff_s16_44k_stereo.aiff", sample_rate: 44_100, frequencies: STEREO, frames: 4_410, container: "aiff", codec: "pcm_s16be", bits: Some(16), tolerance: STEP_16 },
    Fixture { file: "aiff_s24_48k_stereo.aiff", sample_rate: 48_000, frequencies: STEREO, frames: 4_800, container: "aiff", codec: "pcm_s24be", bits: Some(24), tolerance: STEP_24 },
    Fixture { file: "aifc_sowt_44k_stereo.aifc", sample_rate: 44_100, frequencies: STEREO, frames: 4_410, container: "aiff", codec: "pcm_s16le", bits: Some(16), tolerance: STEP_16 },
    Fixture { file: "flac_s16_44k_stereo.flac", sample_rate: 44_100, frequencies: STEREO, frames: 11_025, container: "flac", codec: "flac", bits: Some(16), tolerance: STEP_16 },
    Fixture { file: "flac_s24_48k_mono.flac", sample_rate: 48_000, frequencies: MONO, frames: 12_000, container: "flac", codec: "flac", bits: Some(24), tolerance: STEP_24 },
    Fixture { file: "mp3_44k_stereo.mp3", sample_rate: 44_100, frequencies: STEREO, frames: 11_025, container: "mp3", codec: "mp3", bits: None, tolerance: LOSSY },
    Fixture { file: "mp3_22k_mono.mp3", sample_rate: 22_050, frequencies: MONO, frames: 5_513, container: "mp3", codec: "mp3", bits: None, tolerance: LOSSY },
    Fixture { file: "vorbis_44k_stereo.ogg", sample_rate: 44_100, frequencies: STEREO, frames: 11_025, container: "ogg", codec: "vorbis", bits: None, tolerance: LOSSY },
    Fixture { file: "vorbis_48k_mono.ogg", sample_rate: 48_000, frequencies: MONO, frames: 12_000, container: "ogg", codec: "vorbis", bits: None, tolerance: LOSSY },
];

impl Fixture {
    fn channels(&self) -> u16 {
        self.frequencies.len() as u16
    }

    /// The file ffmpeg wrote to a pipe has no length in its header.
    fn states_its_length(&self) -> bool {
        self.file != "wav_unknown_length.wav"
    }

    fn decoded_bytes(&self) -> u64 {
        (self.frames * self.frequencies.len() * 4) as u64
    }
}

#[test]
fn every_fixture_file_has_an_entry() {
    let mut listed: Vec<&str> = FIXTURES.iter().map(|fixture| fixture.file).collect();
    listed.sort_unstable();
    let on_disk: Vec<String> = fixture_files()
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(listed, on_disk);
}

#[test]
fn fixtures_decode_to_the_tones_they_were_made_from() {
    for fixture in FIXTURES {
        let file = fixture.file;
        let buffer = decode_file(crate::common::fixture(file)).unwrap();
        assert_eq!(buffer.sample_rate(), fixture.sample_rate, "{file}");
        assert_eq!(buffer.channels(), fixture.channels(), "{file}");
        assert_eq!(buffer.frames(), fixture.frames, "{file}");

        for (index, &frequency) in fixture.frequencies.iter().enumerate() {
            let signal = channel(&buffer, index);
            assert_eq!(
                dominant_frequency(&signal, buffer.sample_rate()),
                frequency,
                "{file} channel {index}"
            );
        }

        let formula = tones(
            fixture.sample_rate,
            fixture.frequencies,
            fixture.frames,
            FIXTURE_LEVEL,
        );
        let difference = max_difference(buffer.samples(), formula.samples());
        assert!(
            difference <= fixture.tolerance,
            "{file} is {difference} away from its formula"
        );
    }
}

#[test]
fn probing_reads_the_headers() {
    for fixture in FIXTURES {
        let file = fixture.file;
        let info = probe_file(crate::common::fixture(file)).unwrap();
        assert_eq!(info.sample_rate, fixture.sample_rate, "{file}");
        assert_eq!(info.channels, fixture.channels(), "{file}");
        assert_eq!(info.format, fixture.container, "{file}");
        assert_eq!(info.codec, fixture.codec, "{file}");
        assert_eq!(info.bits_per_sample, fixture.bits, "{file}");

        if fixture.states_its_length() {
            assert_eq!(info.frames, Some(fixture.frames as u64), "{file}");
            let seconds = fixture.frames as f64 / f64::from(fixture.sample_rate);
            let stated = info.duration_secs.unwrap();
            assert!((stated - seconds).abs() < 1.0e-9, "{file}: {stated}");
        } else {
            assert_eq!(info.frames, None, "{file}");
            assert_eq!(info.duration_secs, None, "{file}");
        }
    }
}

#[test]
fn bytes_decode_and_probe_the_same_as_files() {
    for fixture in FIXTURES {
        let file = fixture.file;
        let path = crate::common::fixture(file);
        let bytes = fs::read(&path).unwrap();
        let from_file = decode_file(&path).unwrap();
        let extension = path.extension().unwrap().to_str().unwrap();

        // The hint may be right, absent or wrong.
        for hint in [Some(extension), None, Some("mp3"), Some("wav"), Some("txt")] {
            let from_bytes = decode_bytes(&bytes, hint).unwrap();
            assert_eq!(from_bytes.sample_rate(), from_file.sample_rate(), "{file}");
            assert_eq!(from_bytes.channels(), from_file.channels(), "{file}");
            assert_eq!(from_bytes.samples(), from_file.samples(), "{file}");
            assert_eq!(
                probe_bytes(&bytes, hint).unwrap(),
                probe_file(&path).unwrap(),
                "{file}"
            );
        }
    }
}

#[test]
fn a_file_with_the_wrong_extension_still_decodes() {
    let dir = TempDir::new("wrong-extension");
    let path = dir.path("really-a-flac.wav");
    fs::copy(fixture("flac_s16_44k_stereo.flac"), &path).unwrap();
    assert_eq!(probe_file(&path).unwrap().format, "flac");
    assert_eq!(decode_file(&path).unwrap().frames(), 11_025);
}

#[test]
fn audio_over_the_size_limit_is_refused() {
    for fixture in FIXTURES {
        let file = fixture.file;
        let path = crate::common::fixture(file);

        let exact = DecodeOptions {
            max_decoded_bytes: fixture.decoded_bytes(),
        };
        let buffer = decode_file_with(&path, &exact).unwrap();
        assert_eq!(buffer.frames(), fixture.frames, "{file}");

        let one_short = DecodeOptions {
            max_decoded_bytes: fixture.decoded_bytes() - 1,
        };
        let error = decode_file_with(&path, &one_short).unwrap_err();
        assert!(
            matches!(error, CodecError::TooLarge { limit_bytes } if limit_bytes == one_short.max_decoded_bytes),
            "{file}: {error}"
        );
    }
}

#[test]
fn the_size_limit_message_names_the_limit() {
    let error = CodecError::TooLarge {
        limit_bytes: 3 * 1024 * 1024,
    };
    let message = windfall_core::english::english(&error.to_string());
    assert!(message.contains("too long"), "{message}");
    assert!(message.contains("3 MB"), "{message}");
}

#[test]
fn an_mp3_behind_a_large_tag_still_decodes() {
    // Cover art often makes the ID3 tag larger than the megabyte that format
    // detection is willing to search, so the tag has to be read as a tag.
    let bytes = fs::read(fixture("mp3_44k_stereo.mp3")).unwrap();
    assert_eq!(&bytes[..3], b"ID3");
    let own_tag = 20;

    // An ID3v2.4 header for 2 MB of padding. The size is stored seven bits
    // to a byte.
    let mut tagged = b"ID3\x04\x00\x00\x01\x00\x00\x00".to_vec();
    tagged.resize(10 + (1 << 21), 0);
    tagged.extend(&bytes[own_tag..]);

    let plain = decode_bytes(&bytes, None).unwrap();
    for hint in [Some("mp3"), None] {
        let decoded = decode_bytes(&tagged, hint).unwrap();
        assert_eq!(decoded.sample_rate(), 44_100);
        assert_eq!(decoded.channels(), 2);
        assert!(decoded.samples() == plain.samples());
    }
}
