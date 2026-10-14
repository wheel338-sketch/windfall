//! `Encoder`: every format written the same way.

use std::fs;

use windfall_codec::{
    AudioFormat, CodecError, Encoder, EncoderSettings, FlacBitDepth, WavSampleFormat, decode_file,
    probe_file,
};

use crate::common::*;

/// One of each format, with the codec name a probe of its file gives.
fn every_format() -> Vec<(EncoderSettings, &'static str)> {
    vec![
        (
            EncoderSettings::Wav {
                format: WavSampleFormat::Float32,
            },
            "pcm_f32le",
        ),
        (
            EncoderSettings::Flac {
                depth: FlacBitDepth::Int24,
                level: 5,
            },
            "flac",
        ),
    ]
}

#[test]
fn every_format_is_written_the_same_way_and_reads_back() {
    let dir = TempDir::new("encoder");
    let original = tones(48_000, &[440.0, 1_000.0], 24_000, 0.5);
    for (settings, codec) in every_format() {
        let format = settings.format();
        let name = format!("tone.{}", format.extension());
        let path = dir.path(&name);
        settings.check(48_000, 2).unwrap();

        let mut encoder = Encoder::open(&path, &settings, 48_000, 2).unwrap();
        for block in original.samples().chunks(2 * 1_000) {
            encoder.write(block).unwrap();
        }
        // Nothing has the file's name until it is whole.
        assert!(!path.exists(), "{name}");
        encoder.finalize().unwrap();
        assert_eq!(dir.entries(), [name.as_str()]);

        let info = probe_file(&path).unwrap();
        assert_eq!((info.sample_rate, info.channels), (48_000, 2), "{name}");
        assert_eq!(info.codec, codec, "{name}");
        let decoded = decode_file(&path).unwrap();
        assert_eq!(decoded.frames(), original.frames(), "{name}");
        for side in 0..2 {
            let heard = dominant_frequency(&channel(&decoded, side), 48_000);
            assert_eq!(heard, [440.0, 1_000.0][side], "{name}");
        }

        // The same audio gives the same bytes.
        let first = fs::read(&path).unwrap();
        let mut encoder = Encoder::open(&path, &settings, 48_000, 2).unwrap();
        encoder.write(original.samples()).unwrap();
        encoder.finalize().unwrap();
        assert!(fs::read(&path).unwrap() == first, "{name}");
        fs::remove_file(&path).unwrap();
    }
}

#[test]
fn an_encoder_that_is_dropped_leaves_nothing_behind() {
    let dir = TempDir::new("encoder-dropped");
    for (settings, _) in every_format() {
        let path = dir.path("gone");
        let mut encoder = Encoder::open(&path, &settings, 44_100, 1).unwrap();
        encoder.write(&[0.25; 30_000]).unwrap();
        drop(encoder);
        assert!(dir.entries().is_empty(), "{:?}", settings.format());
    }
}

#[test]
fn each_format_has_its_ending_and_its_name() {
    let named = [
        (AudioFormat::Wav, "wav", "WAV"),
        (AudioFormat::Flac, "flac", "FLAC"),
    ];
    for (format, extension, name) in named {
        assert_eq!(format.extension(), extension);
        assert_eq!(format.name(), name);
        assert!(windfall_codec::is_audio_extension(extension));
    }
}

#[test]
fn what_cannot_be_written_is_refused_before_a_file_is_made() {
    let dir = TempDir::new("encoder-refused");
    let path = dir.path("out");
    let refused = |settings: EncoderSettings, sample_rate: u32, channels: u16| {
        let checked = settings
            .check(sample_rate, channels)
            .unwrap_err()
            .to_string();
        let opened = Encoder::open(&path, &settings, sample_rate, channels)
            .unwrap_err()
            .to_string();
        assert_eq!(checked, opened);
        let checked = windfall_core::english::english(&checked);
        assert!(matches!(
            settings.check(sample_rate, channels),
            Err(CodecError::InvalidInput(_))
        ));
        checked
    };
    let wav = EncoderSettings::Wav {
        format: WavSampleFormat::Int16,
    };
    let flac = |level: u8| EncoderSettings::Flac {
        depth: FlacBitDepth::Int16,
        level,
    };
    assert_eq!(
        refused(wav, 0, 2),
        "cannot write the audio file: the sample rate is zero"
    );
    assert_eq!(
        refused(flac(5), 48_000, 0),
        "cannot write the audio file: there are no channels"
    );
    assert_eq!(
        refused(flac(5), 48_000, 12),
        "cannot write the audio file: a FLAC file cannot have more than 8 channels, and 12 were asked for"
    );
    assert_eq!(
        refused(flac(12), 48_000, 2),
        "cannot write the audio file: FLAC compression levels go from 0 to 8, and 12 was asked for"
    );
    assert!(dir.entries().is_empty());
}
