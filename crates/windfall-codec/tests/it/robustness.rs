//! Feeds the decoder empty, foreign, cut-off and damaged input, and headers
//! that claim more than any real file holds. Every call must come back with
//! a buffer or an error; a panic fails the test. A claim must not cost
//! memory either.

use std::fs;

use windfall_codec::{
    CodecError, DecodeOptions, decode_bytes, decode_bytes_with, decode_file, probe_bytes,
    probe_file,
};
use windfall_core::AudioBuffer;

use crate::common::*;
use crate::heap::peak_heap;

/// Most heap the decoder may hold while it turns a file down for what its
/// header claims: room for the buffers the file is read through, and for
/// nothing sized by the claim.
const REFUSAL_CEILING: usize = 1024 * 1024;

/// Every fixture's name, bytes and full decode.
fn fixtures() -> Vec<(String, Vec<u8>, AudioBuffer)> {
    fixture_files()
        .into_iter()
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let bytes = fs::read(&path).unwrap();
            let decoded = decode_file(&path).unwrap();
            (name, bytes, decoded)
        })
        .collect()
}

/// Decodes and probes `bytes`. The results may be errors, but a buffer that
/// does come back has to be one the engine can use.
fn decode_any(bytes: &[u8], hint: Option<&str>) -> Result<AudioBuffer, CodecError> {
    let _ = probe_bytes(bytes, hint);
    let decoded = decode_bytes(bytes, hint);
    if let Ok(buffer) = &decoded {
        assert!(buffer.frames() > 0);
        assert!(buffer.sample_rate() > 0);
        assert!(buffer.channels() > 0);
        assert!(buffer.samples().iter().all(|sample| sample.is_finite()));
    }
    decoded
}

#[test]
fn empty_input_holds_no_audio() {
    let dir = TempDir::new("empty");
    let path = dir.path("empty.wav");
    fs::write(&path, b"").unwrap();

    assert!(matches!(decode_file(&path), Err(CodecError::NoAudio)));
    assert!(matches!(probe_file(&path), Err(CodecError::NoAudio)));
    assert!(matches!(
        decode_bytes(&[], Some("wav")),
        Err(CodecError::NoAudio)
    ));
    assert!(matches!(probe_bytes(&[], None), Err(CodecError::NoAudio)));
}

#[test]
fn a_missing_file_or_a_folder_is_an_io_error() {
    let dir = TempDir::new("missing");
    let missing = dir.path("no-such-file.wav");
    assert!(matches!(decode_file(&missing), Err(CodecError::Io(_))));
    assert!(matches!(probe_file(&missing), Err(CodecError::Io(_))));

    let folder = dir.path("folder.wav");
    fs::create_dir(&folder).unwrap();
    assert!(decode_file(&folder).is_err());
    assert!(probe_file(&folder).is_err());
}

#[test]
fn files_that_are_not_audio_are_unsupported() {
    let script = fs::read(fixture("generate.sh")).unwrap();
    let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
    let cases: [(&str, &[u8]); 6] = [
        ("a shell script", &script),
        (
            "json",
            br#"{"name": "My Song", "tempo": 140.0, "channels": []}"#,
        ),
        ("a png header", &png),
        ("zeros", &[0; 4096]),
        ("one byte", b"R"),
        (
            "an avi",
            b"RIFF\x24\x00\x00\x00AVI LIST\x10\x00\x00\x00hdrlavih",
        ),
    ];
    for (what, bytes) in cases {
        for hint in [None, Some("wav"), Some("mp3")] {
            let error = decode_any(bytes, hint).unwrap_err();
            assert!(
                matches!(error, CodecError::UnsupportedFormat(_)),
                "{what}: {error:?}"
            );
            let error = probe_bytes(bytes, hint).unwrap_err();
            assert!(
                matches!(error, CodecError::UnsupportedFormat(_)),
                "{what}: {error:?}"
            );
        }
    }
}

#[test]
fn error_messages_read_as_sentences() {
    let not_audio = decode_bytes(b"plain text, not audio", None).unwrap_err();
    assert_eq!(
        windfall_core::english::english(&not_audio.to_string()),
        "this is not a supported audio file (the format was not recognized)"
    );

    let bytes = riff_wave(&[adpcm_fmt_chunk(), chunk(b"data", &[0; 512])]);
    assert_eq!(
        windfall_core::english::english(&decode_bytes(&bytes, None).unwrap_err().to_string()),
        OTHER_CODEC
    );
    assert_eq!(
        windfall_core::english::english(&probe_bytes(&bytes, None).unwrap_err().to_string()),
        OTHER_CODEC
    );

    let cut_short = decode_bytes(b"RIFF\x24\x00\x00\x00WAVEfmt ", None).unwrap_err();
    assert_eq!(
        windfall_core::english::english(&cut_short.to_string()),
        "the audio file is damaged (the file ends unexpectedly)"
    );
    assert_eq!(
        windfall_core::english::english(&CodecError::NoAudio.to_string()),
        "the file contains no audio"
    );
}

#[test]
fn random_bytes_never_panic() {
    let mut rng = Rng::new(0x5EED);
    // Bare noise, and noise behind each format's opening bytes so that the
    // readers get past their first check.
    let openings: [&[u8]; 8] = [
        b"",
        b"RIFF\xF0\xFF\x00\x00WAVEfmt ",
        b"RIFF\xF0\xFF\x00\x00WAVEdata",
        b"FORM\x00\x00\xFF\xF0AIFFCOMM",
        b"fLaC\x00\x00\x00\x22",
        b"OggS\x00\x02",
        b"ID3\x04\x00\x00\x00\x00\x00\x10",
        b"\xFF\xFB\x90\x00",
    ];
    for opening in openings {
        for _ in 0..150 {
            let mut bytes = opening.to_vec();
            let noise = rng.below(6_000);
            bytes.extend((0..noise).map(|_| rng.next() as u8));
            let _ = decode_any(&bytes, None);
        }
    }
}

#[test]
fn cut_off_files_give_their_first_part_or_an_error() {
    for (name, bytes, full) in fixtures() {
        let width = usize::from(full.channels());
        let mut cuts: Vec<usize> = (1..96).collect();
        cuts.extend((1..64).map(|step| bytes.len() * step / 64));
        cuts.extend([bytes.len() - 3, bytes.len() - 2, bytes.len() - 1]);

        for cut in cuts {
            let Ok(part) = decode_any(&bytes[..cut], None) else {
                continue;
            };
            assert_eq!(part.sample_rate(), full.sample_rate(), "{name} at {cut}");
            assert_eq!(part.channels(), full.channels(), "{name} at {cut}");
            assert!(part.frames() <= full.frames(), "{name} at {cut}");

            // What comes back is exactly the start of the real audio.
            let same_span = &full.samples()[..part.frames() * width];
            assert!(part.samples() == same_span, "{name} at {cut}");
        }

        // Losing the last byte costs at most the last block of a compressed
        // file, never the whole file.
        let nearly_all = decode_any(&bytes[..bytes.len() - 1], None).unwrap();
        assert!(
            nearly_all.frames() * 10 >= full.frames() * 7,
            "{name}: {} of {} frames",
            nearly_all.frames(),
            full.frames()
        );
    }
}

#[test]
fn damaged_files_never_panic() {
    let mut rng = Rng::new(0xDA_4A6E);
    for (_, bytes, _) in fixtures() {
        for round in 0..80 {
            let mut damaged = bytes.clone();
            // Half of the rounds hit the headers, where the parsers decide
            // how much to trust the rest.
            let reach = if round % 2 == 0 { 96 } else { bytes.len() };
            let start = rng.below(reach);
            let length = (1 + rng.below(48)).min(damaged.len() - start);
            for byte in &mut damaged[start..start + length] {
                *byte = match round % 3 {
                    0 => rng.next() as u8,
                    1 => 0x00,
                    _ => 0xFF,
                };
            }
            let _ = decode_any(&damaged, None);
        }
    }
}

#[test]
fn a_second_stream_glued_on_is_left_out() {
    // One buffer has one sample rate and channel count, so a file that
    // switches part-way gives only its first stream.
    for (first, second) in [
        ("mp3_44k_stereo.mp3", "mp3_22k_mono.mp3"),
        ("mp3_22k_mono.mp3", "mp3_44k_stereo.mp3"),
        ("vorbis_44k_stereo.ogg", "vorbis_48k_mono.ogg"),
        ("vorbis_48k_mono.ogg", "vorbis_44k_stereo.ogg"),
    ] {
        let alone = decode_file(fixture(first)).unwrap();
        let mut bytes = fs::read(fixture(first)).unwrap();
        bytes.extend(fs::read(fixture(second)).unwrap());

        let glued = decode_any(&bytes, None).unwrap();
        assert_eq!(glued.sample_rate(), alone.sample_rate(), "{first}");
        assert_eq!(glued.channels(), alone.channels(), "{first}");
        assert!(glued.samples() == alone.samples(), "{first}");
    }
}

/// One RIFF chunk, padded to an even length as the format requires.
fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut bytes = id.to_vec();
    bytes.extend((body.len() as u32).to_le_bytes());
    bytes.extend(body);
    if body.len() % 2 == 1 {
        bytes.push(0);
    }
    bytes
}

fn riff_wave(chunks: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = chunks.concat();
    let mut bytes = b"RIFF".to_vec();
    bytes.extend((body.len() as u32 + 4).to_le_bytes());
    bytes.extend(b"WAVE");
    bytes.extend(body);
    bytes
}

/// Overwrites the length field of a chunk or of the whole RIFF file.
fn claim_length(header: &mut [u8], length: u32) {
    header[4..8].copy_from_slice(&length.to_le_bytes());
}

/// A 16-bit PCM `fmt ` chunk. The byte rate and the frame size wrap around
/// where the channel count or sample rate is too large for their fields.
fn fmt_chunk(channels: u16, sample_rate: u32) -> Vec<u8> {
    let frame = channels.wrapping_mul(2);
    let mut body = 1_u16.to_le_bytes().to_vec();
    body.extend(channels.to_le_bytes());
    body.extend(sample_rate.to_le_bytes());
    body.extend(sample_rate.wrapping_mul(u32::from(frame)).to_le_bytes());
    body.extend(frame.to_le_bytes());
    body.extend(16_u16.to_le_bytes());
    chunk(b"fmt ", &body)
}

const OTHER_CODEC: &str =
    "this is not a supported audio file (it uses a codec Windfall cannot read)";

/// The `fmt ` chunk of a WAV holding IMA ADPCM, which is not among the
/// codecs built in.
fn adpcm_fmt_chunk() -> Vec<u8> {
    let mut body = 0x0011_u16.to_le_bytes().to_vec();
    body.extend(1_u16.to_le_bytes());
    body.extend(22_050_u32.to_le_bytes());
    body.extend(11_100_u32.to_le_bytes());
    // 256-byte blocks of 4-bit samples, 505 samples to a block.
    body.extend(256_u16.to_le_bytes());
    body.extend(4_u16.to_le_bytes());
    body.extend(2_u16.to_le_bytes());
    body.extend(505_u16.to_le_bytes());
    chunk(b"fmt ", &body)
}

/// A rising ramp, one step per sample, as 16-bit PCM bytes.
fn ramp(samples: i16) -> Vec<u8> {
    (0..samples)
        .flat_map(|value| (value * 100).to_le_bytes())
        .collect()
}

fn ramp_decoded(samples: i16) -> Vec<f32> {
    (0..samples)
        .map(|value| f32::from(value * 100) / 32_768.0)
        .collect()
}

#[test]
fn chunks_around_the_audio_are_skipped() {
    let bytes = riff_wave(&[
        chunk(b"JUNK", &[0; 28]),
        chunk(b"bext", b"odd length body"),
        fmt_chunk(2, 32_000),
        chunk(b"cue ", &[0; 4]),
        chunk(b"data", &ramp(200)),
        chunk(b"LIST", b"INFOISFT\x06\x00\x00\x00tests\x00"),
        chunk(b"id3 ", b"ID3 not really"),
    ]);
    let buffer = decode_any(&bytes, None).unwrap();
    assert_eq!(buffer.sample_rate(), 32_000);
    assert_eq!(buffer.channels(), 2);
    assert_eq!(buffer.samples(), ramp_decoded(200));
    assert_eq!(probe_bytes(&bytes, None).unwrap().frames, Some(100));
}

#[test]
fn a_header_that_overstates_the_length_gives_the_audio_that_is_there() {
    // A recorder that was cut off: the header was written for almost 2 GB
    // of samples, and 600 bytes follow.
    let mut data = chunk(b"data", &ramp(300));
    claim_length(&mut data, 0x7FFF_FFF0);
    let mut bytes = riff_wave(&[fmt_chunk(1, 8_000), data]);
    claim_length(&mut bytes, 0x7FFF_FFF0 + 36);

    assert_eq!(probe_bytes(&bytes, None).unwrap().frames, Some(0x3FFF_FFF8));
    let buffer = decode_any(&bytes, None).unwrap();
    assert_eq!(buffer.samples(), ramp_decoded(300));

    // The claim is not believed, so only the real audio counts to the limit.
    let fits = DecodeOptions {
        max_decoded_bytes: 300 * 4,
    };
    assert_eq!(
        decode_bytes_with(&bytes, None, &fits).unwrap().frames(),
        300
    );
    let too_small = DecodeOptions {
        max_decoded_bytes: 300 * 4 - 1,
    };
    assert!(matches!(
        decode_bytes_with(&bytes, None, &too_small),
        Err(CodecError::TooLarge { .. })
    ));
}

#[test]
fn headers_with_impossible_values_are_errors() {
    let audio = chunk(b"data", &ramp(64));
    let mut no_bits = fmt_chunk(2, 44_100);
    no_bits[22..24].copy_from_slice(&0_u16.to_le_bytes());

    let cases = [
        (
            "no channels",
            riff_wave(&[fmt_chunk(0, 44_100), audio.clone()]),
        ),
        (
            "no sample rate",
            riff_wave(&[fmt_chunk(2, 0), audio.clone()]),
        ),
        ("no bits", riff_wave(&[no_bits, audio.clone()])),
        ("no format", riff_wave(std::slice::from_ref(&audio))),
        ("no data", riff_wave(&[fmt_chunk(2, 44_100)])),
        (
            "empty data",
            riff_wave(&[fmt_chunk(2, 44_100), chunk(b"data", &[])]),
        ),
        (
            "half a frame",
            riff_wave(&[fmt_chunk(2, 44_100), chunk(b"data", &[1, 2])]),
        ),
    ];
    for (what, bytes) in cases {
        assert!(decode_any(&bytes, None).is_err(), "{what} decoded");
    }
}

fn unsupported(reason: &str) -> String {
    format!("this is not a supported audio file ({reason})")
}

/// Checks that `bytes` are turned down with `message`, when probed and when
/// decoded with next to no memory allowed, and that neither holds more than
/// `REFUSAL_CEILING` of heap on the way.
fn assert_refused_cheaply(bytes: &[u8], message: &str) {
    let tiny = DecodeOptions {
        max_decoded_bytes: 4,
    };
    // The first run fills the tables that are built once per process.
    let _ = decode_bytes_with(bytes, None, &tiny);

    let (decoded, decoding) = peak_heap(|| decode_bytes_with(bytes, None, &tiny));
    assert_eq!(
        windfall_core::english::english(&decoded.unwrap_err().to_string()),
        message
    );
    assert!(
        decoding <= REFUSAL_CEILING,
        "decoding held {decoding} bytes"
    );

    let (probed, probing) = peak_heap(|| probe_bytes(bytes, None));
    assert_eq!(
        windfall_core::english::english(&probed.unwrap_err().to_string()),
        message
    );
    assert!(probing <= REFUSAL_CEILING, "probing held {probing} bytes");
}

/// One AIFF chunk, padded to an even length as the format requires.
fn aiff_chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut bytes = id.to_vec();
    bytes.extend((body.len() as u32).to_be_bytes());
    bytes.extend(body);
    if body.len() % 2 == 1 {
        bytes.push(0);
    }
    bytes
}

/// A whole number of hertz as the 80-bit float an AIFF states its rate in.
fn extended(sample_rate: u32) -> [u8; 10] {
    let mut bytes = [0; 10];
    if sample_rate > 0 {
        let zeros = sample_rate.leading_zeros();
        bytes[..2].copy_from_slice(&(16_383 + 31 - zeros as u16).to_be_bytes());
        bytes[2..6].copy_from_slice(&(sample_rate << zeros).to_be_bytes());
    }
    bytes
}

/// An AIFF, or AIFF-C if `form` says so, with `sound` as its audio in
/// samples of `bits` bits.
fn aiff(form: &[u8; 4], channels: u16, bits: u16, sample_rate: [u8; 10], sound: &[u8]) -> Vec<u8> {
    let frames = sound.len() / usize::from(bits / 8) / usize::from(channels);
    let mut common = channels.to_be_bytes().to_vec();
    common.extend((frames as u32).to_be_bytes());
    common.extend(bits.to_be_bytes());
    common.extend(sample_rate);
    if form == b"AIFC" {
        // Not compressed, under a name of no letters.
        common.extend(b"NONE\0\0");
    }
    // No offset and no block size, then the samples.
    let mut sound_data = vec![0; 8];
    sound_data.extend(sound);

    let body = [
        aiff_chunk(b"COMM", &common),
        aiff_chunk(b"SSND", &sound_data),
    ]
    .concat();
    let mut bytes = b"FORM".to_vec();
    bytes.extend((body.len() as u32 + 4).to_be_bytes());
    bytes.extend(form);
    bytes.extend(body);
    bytes
}

/// A FLAC fixture with another sample rate written into its stream info.
fn flac_claiming(sample_rate: u32) -> Vec<u8> {
    let mut bytes = fs::read(fixture("flac_s16_44k_stereo.flac")).unwrap();
    assert_eq!(&bytes[..5], b"fLaC\0", "the stream info comes first");
    // The rate is 20 bits long and starts 18 bytes in. The four bits behind
    // it belong to the channel count and the sample size.
    bytes[18] = (sample_rate >> 12) as u8;
    bytes[19] = (sample_rate >> 4) as u8;
    bytes[20] = (sample_rate << 4) as u8 | (bytes[20] & 0x0F);
    bytes
}

/// A Vorbis fixture with another channel count and sample rate written into
/// its identification header.
fn vorbis_claiming(channels: u8, sample_rate: u32) -> Vec<u8> {
    let mut bytes = fs::read(fixture("vorbis_44k_stereo.ogg")).unwrap();
    // The header has the first page to itself: 28 bytes of page header, then
    // the packet's name, its version and the two values.
    assert_eq!(&bytes[28..35], b"\x01vorbis");
    bytes[39] = channels;
    bytes[40..44].copy_from_slice(&sample_rate.to_le_bytes());
    bytes[22..26].fill(0);
    let checksum = ogg_checksum(&bytes[..58]);
    bytes[22..26].copy_from_slice(&checksum.to_le_bytes());
    bytes
}

#[test]
fn a_tiny_aiff_claiming_thousands_of_channels_is_turned_down_cheaply() {
    // A decoder made for this header would set aside 1152 frames of 16,000
    // channels at four bytes a sample, 74 MB, for a file of 118 bytes.
    let bytes = aiff(b"AIFF", 16_000, 32, extended(44_100), &[0; 64]);
    assert_eq!(bytes.len(), 118);
    assert_refused_cheaply(&bytes, &unsupported("it has 16000 channels"));

    let compressed = aiff(b"AIFC", u16::MAX, 16, extended(44_100), &[0; 64]);
    assert_refused_cheaply(&compressed, &unsupported("it has 65535 channels"));
}

#[test]
fn a_wav_claiming_thousands_of_channels_is_turned_down_cheaply() {
    let audio = chunk(b"data", &ramp(64));
    let plain = fmt_chunk(60_000, 44_100);
    let bytes = riff_wave(&[plain.clone(), audio.clone()]);
    assert_refused_cheaply(&bytes, &unsupported("it has 60000 channels"));

    // The extensible form of the chunk, which adds a channel mask that the
    // reader fits to the channel count.
    let mut extensible = plain[8..].to_vec();
    extensible[..2].copy_from_slice(&0xFFFE_u16.to_le_bytes());
    extensible.extend(22_u16.to_le_bytes());
    extensible.extend(16_u16.to_le_bytes());
    extensible.extend(0_u32.to_le_bytes());
    // The ID of PCM.
    extensible.extend([
        1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0x38, 0x9B, 0x71,
    ]);
    let bytes = riff_wave(&[chunk(b"fmt ", &extensible), audio]);
    assert_refused_cheaply(&bytes, &unsupported("it has 60000 channels"));
}

#[test]
fn a_vorbis_stream_claiming_every_channel_it_can_is_turned_down_cheaply() {
    // FLAC and MP3 have no way to state more than eight and two channels.
    assert_refused_cheaply(
        &vorbis_claiming(255, 44_100),
        &unsupported("it has 255 channels"),
    );
    assert_refused_cheaply(
        &vorbis_claiming(65, 44_100),
        &unsupported("it has 65 channels"),
    );
}

#[test]
fn sample_rates_no_file_has_are_refused() {
    let too_fast = |rate: u32| unsupported(&format!("it has a sample rate of {rate} Hz"));
    let audio = chunk(b"data", &ramp(64));
    let wav = |rate| riff_wave(&[fmt_chunk(2, rate), audio.clone()]);

    assert_refused_cheaply(&wav(768_001), &too_fast(768_001));
    assert_refused_cheaply(&wav(u32::MAX), &too_fast(u32::MAX));
    assert_refused_cheaply(
        &wav(0),
        "the audio file is damaged (the sample rate is zero)",
    );

    let sound = [0; 64];
    let megahertz = aiff(b"AIFF", 2, 16, extended(1_000_000), &sound);
    assert_refused_cheaply(&megahertz, &too_fast(1_000_000));
    // Two to the power of 40, which the reader takes for the highest rate
    // it can count to.
    let beyond_counting = [0x40, 0x27, 0x80, 0, 0, 0, 0, 0, 0, 0];
    let compressed = aiff(b"AIFC", 2, 16, beyond_counting, &sound);
    assert_refused_cheaply(&compressed, &too_fast(u32::MAX));

    // The 20 bits a FLAC states its rate in reach a little over 1 MHz.
    assert_refused_cheaply(&flac_claiming(0xF_FFFF), &too_fast(0xF_FFFF));
    assert_refused_cheaply(&flac_claiming(768_001), &too_fast(768_001));
    assert_refused_cheaply(&vorbis_claiming(2, u32::MAX), &too_fast(u32::MAX));
    assert_refused_cheaply(&vorbis_claiming(2, 768_001), &too_fast(768_001));

    // The highest rate allowed is still read.
    let fastest = wav(768_000);
    assert_eq!(probe_bytes(&fastest, None).unwrap().sample_rate, 768_000);
    let buffer = decode_any(&fastest, None).unwrap();
    assert_eq!(buffer.sample_rate(), 768_000);
    assert_eq!(buffer.samples(), ramp_decoded(64));
}

#[test]
fn files_of_many_channels_still_decode() {
    let surround = riff_wave(&[fmt_chunk(8, 48_000), chunk(b"data", &ramp(320))]);
    assert_eq!(probe_bytes(&surround, None).unwrap().channels, 8);
    let buffer = decode_any(&surround, None).unwrap();
    assert_eq!(buffer.channels(), 8);
    assert_eq!(buffer.frames(), 40);
    assert_eq!(buffer.samples(), ramp_decoded(320));

    // As many channels as are allowed. AIFF stores the high byte first.
    let sound: Vec<u8> = (0..320_i16)
        .flat_map(|value| (value * 100).to_be_bytes())
        .collect();
    let widest = aiff(b"AIFF", 64, 16, extended(48_000), &sound);
    assert_eq!(probe_bytes(&widest, None).unwrap().channels, 64);
    let buffer = decode_any(&widest, None).unwrap();
    assert_eq!(buffer.channels(), 64);
    assert_eq!(buffer.frames(), 5);
    assert_eq!(buffer.samples(), ramp_decoded(320));

    let one_more = aiff(b"AIFF", 65, 16, extended(48_000), &sound);
    assert_refused_cheaply(&one_more, &unsupported("it has 65 channels"));
}

#[test]
fn room_for_a_believable_length_is_only_taken_as_the_audio_arrives() {
    // 100,000 samples behind a header written for three million. A file of
    // this size could hold that much if it were compressed, so the length is
    // believed, and comes to 12 MB decoded.
    let sawtooth = |index: usize| (index % 300) as i16 * 100;
    let audio: Vec<u8> = (0..100_000)
        .flat_map(|index| sawtooth(index).to_le_bytes())
        .collect();
    let mut data = chunk(b"data", &audio);
    claim_length(&mut data, 6_000_000);
    let mut bytes = riff_wave(&[fmt_chunk(1, 44_100), data]);
    claim_length(&mut bytes, 6_000_000 + 36);
    assert_eq!(probe_bytes(&bytes, None).unwrap().frames, Some(3_000_000));

    let _ = decode_bytes(&bytes, None);
    let (decoded, peak) = peak_heap(|| decode_bytes(&bytes, None));
    let expected: Vec<f32> = (0..100_000)
        .map(|index| f32::from(sawtooth(index)) / 32_768.0)
        .collect();
    assert!(decoded.unwrap().samples() == expected, "the audio changed");
    // No more than 1 MiB is set aside on the header's word, and the audio
    // that is there fits into that.
    assert!(peak < 1536 * 1024, "held {peak} bytes");
}

#[test]
fn nothing_is_set_aside_for_a_file_without_a_decoder() {
    // 100 kB of ADPCM blocks behind a header written for eight times that:
    // a believable 6 MB once decoded, if there were a decoder for it.
    let mut data = chunk(b"data", &vec![0; 100_000]);
    claim_length(&mut data, 800_000);
    let mut bytes = riff_wave(&[adpcm_fmt_chunk(), data]);
    claim_length(&mut bytes, 800_000 + 40);

    let _ = decode_bytes(&bytes, None);
    let (decoded, peak) = peak_heap(|| decode_bytes(&bytes, None));
    assert_eq!(
        windfall_core::english::english(&decoded.unwrap_err().to_string()),
        OTHER_CODEC
    );
    assert!(peak <= REFUSAL_CEILING, "held {peak} bytes");
}
