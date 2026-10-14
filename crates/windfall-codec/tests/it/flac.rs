//! Writes FLAC files and reads them back. FLAC is lossless, so every test
//! here asks for the exact numbers and not for something close.
//!
//! The numbers a FLAC file must hold are the ones a WAV file of the same
//! bit depth holds: both writers turn samples into integers the same way.

use std::fs;
use std::path::Path;

use windfall_codec::{
    CodecError, DEFAULT_FLAC_LEVEL, FlacBitDepth, FlacWriter, MAX_FLAC_LEVEL, WavSampleFormat,
    decode_file, probe_file, write_flac, write_wav,
};
use windfall_core::AudioBuffer;

use crate::common::*;

const DEPTHS: [(FlacBitDepth, WavSampleFormat, u32); 2] = [
    (FlacBitDepth::Int16, WavSampleFormat::Int16, 16),
    (FlacBitDepth::Int24, WavSampleFormat::Int24, 24),
];

/// Sample rate, channels and length in frames. The lengths sit on both
/// sides of the encoder's block of 4,096 samples and of its smallest ones,
/// and the last four rates are stated in a frame in the four ways a rate
/// that has no code of its own can be.
const LAYOUTS: [(u32, u16, usize); 16] = [
    (44_100, 1, 20_000),
    (48_000, 2, 20_000),
    (96_000, 2, 12_289),
    (48_000, 6, 5_000),
    (44_100, 8, 4_097),
    (48_000, 2, 4_096),
    (48_000, 2, 4_095),
    (48_000, 1, 1),
    (48_000, 2, 2),
    (48_000, 2, 15),
    (48_000, 3, 17),
    (48_000, 2, 8_192),
    (50_000, 2, 3_000),
    (11_025, 1, 3_000),
    (352_800, 2, 3_000),
    (123_457, 2, 5_000),
];

#[derive(Debug, Clone, Copy)]
enum Sound {
    /// A tone on every channel and a little noise: what music looks like
    /// to a predictor.
    Tones,
    /// Two channels that are nearly the same, which sum and difference
    /// store best.
    Centered,
    Silence,
    /// One value throughout.
    Level,
    /// Noise over the whole range and beyond it, which nothing predicts
    /// and which clips.
    Noise,
    /// Silence, a burst, and silence: blocks that change character.
    Burst,
}

const SOUNDS: [Sound; 6] = [
    Sound::Tones,
    Sound::Centered,
    Sound::Silence,
    Sound::Level,
    Sound::Noise,
    Sound::Burst,
];

fn sound(sound: Sound, sample_rate: u32, channels: u16, frames: usize) -> AudioBuffer {
    let frequencies: Vec<f64> = (0..channels)
        .map(|channel| 220.0 * f64::from(channel + 1))
        .collect();
    let mut rng = Rng::new(u64::from(channels) * 1_000 + frames as u64);
    let tones = tones(sample_rate, &frequencies, frames, 0.6);
    let samples = tones
        .samples()
        .chunks(usize::from(channels))
        .enumerate()
        .flat_map(|(frame, tones)| {
            let shared = tones[0];
            let values: Vec<f32> = tones
                .iter()
                .map(|&tone| match sound {
                    Sound::Tones => tone + rng.sample() * 0.05,
                    Sound::Centered => shared + rng.sample() * 0.001,
                    Sound::Silence => 0.0,
                    Sound::Level => 0.25,
                    Sound::Noise => rng.sample() * 1.2,
                    Sound::Burst if (frames / 3..frames / 2).contains(&frame) => {
                        tone + rng.sample() * 0.3
                    }
                    Sound::Burst => 0.0,
                })
                .collect();
            values
        })
        .collect();
    AudioBuffer::from_interleaved(sample_rate, channels, samples)
}

fn bits(buffer: &AudioBuffer) -> Vec<u32> {
    buffer.samples().iter().map(|s| s.to_bits()).collect()
}

/// The samples of a WAV file as they lie in it.
fn wav_data(bytes: &[u8]) -> &[u8] {
    let mut at = 12;
    loop {
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if &bytes[at..at + 4] == b"data" {
            return &bytes[at + 8..at + 8 + size];
        }
        at += 8 + size + size % 2;
    }
}

/// Decodes a FLAC file with Symphonia told to check the audio against the
/// MD5 sum the file states, which is what `flac -t` does. `None` means the
/// decoder did not check.
fn md5_holds(path: &Path) -> Option<bool> {
    use symphonia::core::codecs::audio::AudioDecoderOptions;
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{FormatOptions, TrackType};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;

    let file = fs::File::open(path).unwrap();
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut format = symphonia::default::get_probe()
        .probe(
            &Hint::new(),
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .unwrap();
    let track = format.default_track(TrackType::Audio).unwrap();
    let params = track.codec_params.as_ref().unwrap().audio().unwrap();
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default().verify(true))
        .unwrap();
    while let Some(packet) = format.next_packet().unwrap() {
        decoder.decode(&packet).unwrap();
    }
    decoder.finalize().verify_ok
}

#[test]
fn a_flac_file_holds_exactly_the_numbers_a_wav_file_of_its_depth_holds() {
    let dir = TempDir::new("flac-exact");
    let flac = dir.path("exact.flac");
    let wav = dir.path("exact.wav");
    for (sample_rate, channels, frames) in LAYOUTS {
        for kind in SOUNDS {
            let original = sound(kind, sample_rate, channels, frames);
            for (depth, wav_format, depth_bits) in DEPTHS {
                write_wav(&wav, &original, wav_format).unwrap();
                let expected = decode_file(&wav).unwrap();
                for level in [0, 1, 3, DEFAULT_FLAC_LEVEL, MAX_FLAC_LEVEL] {
                    let label = format!(
                        "{kind:?} at {depth_bits} bits, level {level}, {channels} ch {sample_rate} Hz, {frames} frames"
                    );
                    write_flac(&flac, &original, depth, level).unwrap();

                    let info = probe_file(&flac).unwrap();
                    assert_eq!(info.sample_rate, sample_rate, "{label}");
                    assert_eq!(info.channels, channels, "{label}");
                    assert_eq!(info.frames, Some(frames as u64), "{label}");
                    assert_eq!(info.codec, "flac", "{label}");
                    assert_eq!(info.bits_per_sample, Some(depth_bits), "{label}");

                    let decoded = decode_file(&flac).unwrap();
                    assert_eq!(decoded.sample_rate(), sample_rate, "{label}");
                    assert_eq!(decoded.channels(), channels, "{label}");
                    assert_eq!(decoded.frames(), frames, "{label}");
                    assert!(
                        bits(&decoded) == bits(&expected),
                        "{label}: the audio differs"
                    );
                    assert_eq!(md5_holds(&flac), Some(true), "{label}");
                }
            }
        }
    }
}

#[test]
fn how_the_audio_is_cut_into_writes_does_not_change_the_file() {
    let dir = TempDir::new("flac-streamed");
    let original = sound(Sound::Tones, 48_000, 2, 30_001);
    let whole = dir.path("whole.flac");
    write_flac(&whole, &original, FlacBitDepth::Int24, DEFAULT_FLAC_LEVEL).unwrap();
    let whole = fs::read(whole).unwrap();

    for frames_per_write in [1, 7, 480, 4_095, 4_096, 4_097, 10_000] {
        let path = dir.path("pieces.flac");
        let mut writer =
            FlacWriter::create(&path, 48_000, 2, FlacBitDepth::Int24, DEFAULT_FLAC_LEVEL).unwrap();
        for piece in original.samples().chunks(frames_per_write * 2) {
            writer.write(piece).unwrap();
        }
        writer.write(&[]).unwrap();
        writer.finalize().unwrap();
        assert!(
            fs::read(&path).unwrap() == whole,
            "writes of {frames_per_write} frames gave another file"
        );
    }
}

#[test]
fn every_level_stores_the_same_audio_and_a_higher_one_in_less_room() {
    let dir = TempDir::new("flac-levels");
    // Two seconds of chords with a little hiss, the two sides alike.
    let frames = 96_000;
    let mut rng = Rng::new(7);
    let samples: Vec<f32> = (0..frames)
        .flat_map(|frame| {
            let seconds = frame as f64 / 48_000.0;
            let chord: f64 = [220.0, 277.2, 329.6, 440.0, 1_318.5]
                .iter()
                .map(|frequency| (std::f64::consts::TAU * frequency * seconds).sin() * 0.12)
                .sum();
            let hiss = rng.sample() * 0.002;
            [chord as f32 + hiss, chord as f32 * 0.9 - hiss]
        })
        .collect();
    let original = AudioBuffer::from_interleaved(48_000, 2, samples);

    let mut sizes = Vec::new();
    let mut first: Option<Vec<u32>> = None;
    for level in 0..=MAX_FLAC_LEVEL {
        let path = dir.path("level.flac");
        write_flac(&path, &original, FlacBitDepth::Int16, level).unwrap();
        sizes.push(fs::metadata(&path).unwrap().len());
        let decoded = bits(&decode_file(&path).unwrap());
        assert!(*first.get_or_insert_with(|| decoded.clone()) == decoded);

        // And the same bytes the second time.
        let again = dir.path("again.flac");
        write_flac(&again, &original, FlacBitDepth::Int16, level).unwrap();
        assert!(fs::read(&path).unwrap() == fs::read(&again).unwrap());
    }

    let wav = dir.path("level.wav");
    write_wav(&wav, &original, WavSampleFormat::Int16).unwrap();
    let wav_size = fs::metadata(&wav).unwrap().len();
    assert!(sizes[0] < wav_size * 7 / 10, "{sizes:?} of {wav_size}");
    // The levels that fit a predictor to each block beat the ones that
    // do not, and the highest is the smallest of all.
    assert!(sizes[5] < sizes[2], "{sizes:?}");
    assert!(sizes[1] <= sizes[0], "{sizes:?}");
    assert!(sizes[4] <= sizes[3], "{sizes:?}");
    assert!(sizes[8] <= *sizes.iter().min().unwrap(), "{sizes:?}");
}

#[test]
fn silence_takes_next_to_nothing_and_noise_no_more_than_it_would_as_wav() {
    let dir = TempDir::new("flac-sizes");
    let path = dir.path("size.flac");
    let size = |kind: Sound, depth: FlacBitDepth, level: u8| {
        let original = sound(kind, 48_000, 2, 48_000);
        write_flac(&path, &original, depth, level).unwrap();
        fs::metadata(&path).unwrap().len()
    };
    // 24-bit silence is exactly zero. 16-bit silence carries dither, one
    // step up or down, and still packs to a few bits a sample.
    for level in [0, DEFAULT_FLAC_LEVEL, MAX_FLAC_LEVEL] {
        let silent = size(Sound::Silence, FlacBitDepth::Int16, level);
        assert!(
            silent < 48_000 * 2 * 2 / 5,
            "{silent} bytes at level {level}"
        );
        let level_only = size(Sound::Level, FlacBitDepth::Int24, level);
        assert!(level_only < 48_000 * 2 * 3 / 4, "{level_only} bytes");

        // Noise cannot be packed. It is stored as it is, and all that is
        // added is a few bytes a frame.
        for (depth, bytes) in [(FlacBitDepth::Int16, 2), (FlacBitDepth::Int24, 3)] {
            let noise = size(Sound::Noise, depth, level);
            let raw = 48_000 * 2 * bytes;
            assert!(noise <= raw + raw / 500 + 100, "{noise} bytes for {raw}");
        }
    }
}

#[test]
fn nothing_appears_at_the_destination_before_finalize() {
    let dir = TempDir::new("flac-atomic");
    let path = dir.path("out.flac");
    fs::write(&path, b"the previous export").unwrap();

    let create = || FlacWriter::create(&path, 48_000, 2, FlacBitDepth::Int16, 5).unwrap();
    let mut abandoned = create();
    abandoned.write(&[0.1; 10_000]).unwrap();
    assert_eq!(dir.entries().len(), 2);
    drop(abandoned);
    assert_eq!(dir.entries(), ["out.flac"]);
    assert_eq!(fs::read(&path).unwrap(), b"the previous export");

    let mut writer = create();
    writer.write(&[0.1; 10_000]).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"the previous export");
    writer.finalize().unwrap();
    assert_eq!(dir.entries(), ["out.flac"]);
    assert_eq!(decode_file(&path).unwrap().frames(), 5_000);
}

#[test]
fn what_a_flac_file_cannot_hold_is_refused_in_plain_words() {
    let dir = TempDir::new("flac-refused");
    let path = dir.path("out.flac");
    let refusal = |rate: u32, channels: u16, level: u8| match FlacWriter::create(
        &path,
        rate,
        channels,
        FlacBitDepth::Int16,
        level,
    ) {
        Err(CodecError::InvalidInput(reason)) => windfall_core::english::english(&reason),
        other => panic!("{rate} Hz, {channels} channels, level {level}: {other:?}"),
    };
    assert_eq!(refusal(0, 2, 5), "the sample rate is zero");
    assert_eq!(refusal(48_000, 0, 5), "there are no channels");
    assert_eq!(
        refusal(48_000, 9, 5),
        "a FLAC file cannot have more than 8 channels, and 9 were asked for"
    );
    assert_eq!(
        refusal(768_000, 2, 5),
        "a FLAC file cannot have a sample rate above 655350 Hz, and 768000 Hz was asked for"
    );
    assert_eq!(
        refusal(48_000, 2, 9),
        "FLAC compression levels go from 0 to 8, and 9 was asked for"
    );
    assert!(dir.entries().is_empty());

    let missing = dir.path("no-such-folder").join("out.flac");
    let result = FlacWriter::create(missing, 48_000, 2, FlacBitDepth::Int16, 5);
    assert!(matches!(result, Err(CodecError::Io(_))));

    // A block that splits a frame is refused and leaves the writer usable.
    let mut writer = FlacWriter::create(&path, 48_000, 2, FlacBitDepth::Int16, 5).unwrap();
    assert!(matches!(
        writer.write(&[0.1, 0.2, 0.3]),
        Err(CodecError::InvalidInput(_))
    ));
    writer.write(&[0.1, 0.2]).unwrap();
    writer.finalize().unwrap();
    assert_eq!(decode_file(&path).unwrap().frames(), 1);
}

#[test]
fn a_file_with_no_audio_is_a_header_that_says_so() {
    let dir = TempDir::new("flac-empty");
    let path = dir.path("empty.flac");
    let writer = FlacWriter::create(&path, 44_100, 2, FlacBitDepth::Int16, 5).unwrap();
    writer.finalize().unwrap();
    let bytes = fs::read(&path).unwrap();
    assert_eq!(&bytes[..4], b"fLaC");
    // 44,100 Hz, two channels, 16 bits, and no samples.
    assert_eq!(bytes[18..26], [0x0A, 0xC4, 0x42, 0xF0, 0, 0, 0, 0]);
    // The sum of no audio at all.
    assert_eq!(bytes[26..30], [0xD4, 0x1D, 0x8C, 0xD9]);
    assert_eq!(bytes.len(), 4 + 4 + 34 + 4 + 16);
    assert!(decode_file(&path).is_err());
}

#[test]
fn a_seed_of_its_own_gives_other_dither() {
    let dir = TempDir::new("flac-seed");
    let original = sound(Sound::Tones, 48_000, 2, 5_000);
    let write = |name: &str, seed: Option<u64>| {
        let path = dir.path(name);
        let mut writer = FlacWriter::create(&path, 48_000, 2, FlacBitDepth::Int16, 5).unwrap();
        if let Some(seed) = seed {
            writer = writer.with_dither_seed(seed);
        }
        writer.write(original.samples()).unwrap();
        writer.finalize().unwrap();
        decode_file(&path).unwrap()
    };
    let usual = write("usual.flac", None);
    let other = write("other.flac", Some(1));
    assert!(bits(&usual) != bits(&other));
    assert!(max_difference(usual.samples(), other.samples()) <= 3.0 / 32_768.0);
    assert!(max_difference(usual.samples(), original.samples()) <= 1.5 / 32_768.0);
}

/// Runs ffmpeg and returns what it wrote to its output and what it
/// complained about.
fn ffmpeg(arguments: &[&str]) -> (Vec<u8>, String) {
    let output = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-nostdin", "-v", "warning"])
        .args(arguments)
        .output()
        .expect("ffmpeg has to be on the PATH for this test");
    let complaints = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "ffmpeg failed: {complaints}");
    (output.stdout, complaints)
}

/// The same proof with a decoder that is not ours: ffmpeg, told to stop at
/// the first thing wrong with a frame, gives back the bytes of the WAV
/// file's samples, and the MD5 sum of what it decodes is the one the file
/// states.
#[test]
#[ignore = "needs ffmpeg on the PATH"]
fn ffmpeg_decodes_every_file_to_exactly_what_went_in() {
    let dir = TempDir::new("flac-ffmpeg");
    let flac = dir.path("check.flac");
    let wav = dir.path("check.wav");
    let strict = [
        "-xerror",
        "-err_detect",
        "+crccheck+bitstream+buffer+explode",
    ];
    let mut checked = 0;
    for (sample_rate, channels, frames) in LAYOUTS {
        for kind in SOUNDS {
            let original = sound(kind, sample_rate, channels, frames);
            for (depth, wav_format, depth_bits) in DEPTHS {
                write_wav(&wav, &original, wav_format).unwrap();
                let wav_bytes = fs::read(&wav).unwrap();
                let expected = wav_data(&wav_bytes);
                let (codec, raw) = match depth_bits {
                    16 => ("pcm_s16le", "s16le"),
                    _ => ("pcm_s24le", "s24le"),
                };
                for level in [0, DEFAULT_FLAC_LEVEL, MAX_FLAC_LEVEL] {
                    let label = format!(
                        "{kind:?} at {depth_bits} bits, level {level}, {channels} ch {sample_rate} Hz, {frames} frames"
                    );
                    write_flac(&flac, &original, depth, level).unwrap();
                    let file = flac.to_str().unwrap();

                    let mut decode = strict.to_vec();
                    decode.extend(["-i", file, "-c:a", codec, "-f", raw, "pipe:1"]);
                    let (decoded, complaints) = ffmpeg(&decode);
                    assert_eq!(complaints, "", "{label}");
                    assert!(decoded == expected, "{label}: ffmpeg decoded other audio");

                    let mut sum = strict.to_vec();
                    sum.extend(["-i", file, "-c:a", codec, "-f", "md5", "pipe:1"]);
                    let (sum, _) = ffmpeg(&sum);
                    let stated: String = fs::read(&flac).unwrap()[26..42]
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect();
                    assert_eq!(
                        String::from_utf8_lossy(&sum).trim(),
                        format!("MD5={stated}"),
                        "{label}"
                    );
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, LAYOUTS.len() * SOUNDS.len() * 2 * 3);
}
