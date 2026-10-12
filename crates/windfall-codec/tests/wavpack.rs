//! Original deterministic PCM and hand-constructed public-format blocks.
//! Reference-tool interoperability is an external check, not claimed here.
use std::sync::atomic::{AtomicU32, Ordering};
use windfall_codec::*;
static SERIAL: AtomicU32 = AtomicU32::new(0);
struct Folder(std::path::PathBuf);
impl Folder {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "windfall-wv-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn file(&self, n: &str) -> std::path::PathBuf {
        self.0.join(n)
    }
}
impl Drop for Folder {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn signal(frames: usize, channels: u16, bits: u32, mode: usize) -> Vec<f32> {
    let scale = 1_i32 << (bits - 1);
    let mut seed = 0xabc7_6543_u32;
    (0..frames * usize::from(channels))
        .map(|i| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let integer = match mode {
                0 => 0,
                1 => (seed & ((scale * 2 - 1) as u32)) as i32 - scale,
                2 => ((i as f64 * 0.143 + (i % usize::from(channels)) as f64).sin()
                    * (scale - 1) as f64)
                    .round() as i32,
                3 => scale - 1,
                4 => -scale,
                _ => {
                    if i % 2 == 0 {
                        -scale
                    } else {
                        scale - 1
                    }
                }
            };
            integer as f32 / scale as f32
        })
        .collect()
}
#[test]
fn integer_roundtrips_are_bit_exact_across_signals_rates_lengths_channels_and_partitions() {
    let folder = Folder::new();
    for depth in [WavPackDepth::Int16, WavPackDepth::Int24] {
        let bits = if depth == WavPackDepth::Int16 { 16 } else { 24 };
        for channels in [1, 2] {
            for rate in [1, 8000, 44100, 48000, 96000, 123457] {
                for block in [1, 7, 1024, 4096] {
                    for frames in [0, 1, 17, 4099] {
                        for mode in 0..6 {
                            let pcm = signal(frames, channels, bits, mode);
                            let path = folder.file("roundtrip.wv");
                            let mut writer = WavPackWriter::create_with_tags(
                                &path,
                                rate,
                                channels,
                                depth,
                                block,
                                &AudioTags::default(),
                            )
                            .unwrap();
                            for chunk in pcm.chunks(13 * usize::from(channels)) {
                                writer.write(chunk).unwrap();
                            }
                            writer.finalize().unwrap();
                            let audio = decode_file(&path).unwrap();
                            assert_eq!(
                                (audio.sample_rate(), audio.channels(), audio.frames()),
                                (rate, channels, frames)
                            );
                            assert_eq!(
                                audio.samples(),
                                pcm,
                                "{depth:?}, {channels}, {rate}, {block}, {frames}, {mode}"
                            );
                            let probe = probe_file(&path).unwrap();
                            assert_eq!(probe.frames, Some(frames as u64));
                            assert_eq!(probe.bits_per_sample, Some(bits));
                        }
                    }
                }
            }
        }
    }
}
#[test]
fn headers_wrappers_metadata_and_pcm_crc_have_independent_expected_bytes() {
    let folder = Folder::new();
    let path = folder.file("bytes.wv");
    let mut writer = WavPackWriter::create_with_tags(
        &path,
        12345,
        1,
        WavPackDepth::Int24,
        2,
        &AudioTags {
            title: "Own title",
            ..Default::default()
        },
    )
    .unwrap();
    writer.write(&[0.0, -1.0, 0.5]).unwrap();
    writer.finalize().unwrap();
    let b = std::fs::read(path).unwrap();
    assert_eq!(&b[..4], b"wvpk");
    assert_eq!(&b[8..12], &[0x10, 4, 0, 0]);
    assert_eq!((le(&b, 12), le(&b, 16), le(&b, 20)), (3, 0, 2));
    assert_eq!(le(&b, 24), 2 | 4 | 0x1800 | (23 << 18) | (15 << 23));
    let expected_crc = u32::MAX
        .wrapping_mul(3)
        .wrapping_mul(3)
        .wrapping_add((-8388608_i32) as u32);
    assert_eq!(le(&b, 28), expected_crc);
    assert_eq!(&b[32..34], &[0x21, 22]);
    assert_eq!(&b[34..38], b"RIFF");
    assert_eq!(le(&b, 38), 46); // 36 + nine PCM bytes + RIFF pad
    assert_eq!(&b[42..50], b"WAVEfmt ");
    assert_eq!(le(&b, 58), 12345);
    assert_eq!(le(&b, 74), 9);
    let at = le(&b, 4) as usize + 8;
    assert_eq!(le(&b, at + 16), 2);
    assert_eq!(le(&b, at + 20), 1);
    assert!(b.windows(8).any(|v| v == b"APETAGEX"));
    assert!(b.windows(9).any(|v| v == b"Own title"));
}
fn independent_block(frames: u32, crc: u32, bits: [u8; 2]) -> Vec<u8> {
    let mut b = b"wvpk".to_vec();
    b.extend_from_slice(&36_u32.to_le_bytes());
    b.extend_from_slice(&[0x10, 4, 0, 0]);
    for n in [
        frames,
        0,
        frames,
        1 | 4 | 0x1800 | (15 << 18) | (10 << 23),
        crc,
    ] {
        b.extend_from_slice(&n.to_le_bytes());
    }
    b.extend_from_slice(&[5, 3, 0, 0, 0, 0, 0, 0, 10, 1, bits[0], bits[1]]);
    b
}
#[test]
fn independent_zero_run_and_held_unary_fixtures_decode_to_known_pcm() {
    // Elias gamma(4): 1110 00 (LSB first), followed by one padding bits.
    let silence = independent_block(4, 0xffff_ffaf, [0xc7, 0xff]);
    assert_eq!(decode_bytes(&silence, None).unwrap().samples(), &[0.0; 4]);
    // zero-run escape 0; zone 1 with held-one => 1110; sign 0;
    // next zone 1 reuses that held one => 0; sign 1.
    let pair = independent_block(2, 0xffff_fff8, [0x8e, 0xff]);
    assert_eq!(
        decode_bytes(&pair, Some("WV")).unwrap().samples(),
        &[1.0 / 32768.0, -2.0 / 32768.0]
    );
}

#[test]
fn version_five_checksum_detects_header_damage_and_metadata_only_trailers_preserve_audio() {
    let mut b = independent_block(2, 0xffff_fff8, [0x8e, 0xff]);
    b[4..8].copy_from_slice(&42_u32.to_le_bytes()); // six-byte checksum sub-block
    let flags = le(&b, 24) | 0x1000_0000;
    b[24..28].copy_from_slice(&flags.to_le_bytes());
    let sum = b.as_chunks::<2>().0.iter().fold(u32::MAX, |sum, v| {
        sum.wrapping_mul(3)
            .wrapping_add(u32::from(u16::from_le_bytes([v[0], v[1]])))
    });
    b.extend_from_slice(&[0x2f, 2]);
    b.extend_from_slice(&sum.to_le_bytes());
    assert_eq!(decode_bytes(&b, None).unwrap().frames(), 2);
    let mut damaged = b.clone();
    damaged[27] ^= 8; // reserved flag has no effect on PCM CRC
    assert!(
        decode_bytes(&damaged, None)
            .unwrap_err()
            .to_string()
            .contains("checksum")
    );
    // Reference tools may use zero layout flags for a wrapper-only block.
    let mut trailer = b"wvpk".to_vec();
    trailer.extend_from_slice(&28_u32.to_le_bytes());
    trailer.extend_from_slice(&[0x10, 4, 0, 0]);
    for n in [2_u32, 2, 0, 0, u32::MAX] {
        trailer.extend_from_slice(&n.to_le_bytes());
    }
    trailer.extend_from_slice(&[0x62, 1, 0, 0]); // odd one-byte RIFF trailer
    b.extend_from_slice(&trailer);
    assert_eq!(
        decode_bytes(&b, None).unwrap().samples(),
        &[1.0 / 32768.0, -2.0 / 32768.0]
    );
}
#[test]
fn truncated_crc_sizes_metadata_and_budget_attacks_are_refused_without_panics() {
    let source = independent_block(2, 0xffff_fff8, [0x8e, 0xff]);
    for n in 0..source.len() {
        let result = std::panic::catch_unwind(|| decode_bytes(&source[..n], Some("wv")));
        assert!(result.unwrap().is_err(), "truncation {n}");
    }
    for (at, value) in [(4, u32::MAX), (20, u32::MAX), (28, 0), (24, 0x8000_0000)] {
        let mut bytes = source.clone();
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        assert!(
            std::panic::catch_unwind(|| decode_bytes(&bytes, Some("wv")))
                .unwrap()
                .is_err()
        );
    }
    for at in [32, 33, 40, 41] {
        let mut bytes = source.clone();
        bytes[at] = 255;
        assert!(
            std::panic::catch_unwind(|| decode_bytes(&bytes, Some("wv")))
                .unwrap()
                .is_err()
        );
    }
    assert!(matches!(
        decode_bytes_with(
            &source,
            None,
            &DecodeOptions {
                max_decoded_bytes: 4
            }
        ),
        Err(CodecError::TooLarge { .. })
    ));
    let mut hybrid = source;
    let f = le(&hybrid, 24) | 8;
    hybrid[24..28].copy_from_slice(&f.to_le_bytes());
    assert!(
        decode_bytes(&hybrid, None)
            .unwrap_err()
            .to_string()
            .contains("hybrid")
    );
}
#[test]
fn abandoned_and_failed_wavpack_writers_preserve_destination_and_remove_staging() {
    let folder = Folder::new();
    let path = folder.file("old.wv");
    std::fs::write(&path, b"original").unwrap();
    {
        let mut writer = Encoder::open(
            &path,
            &EncoderSettings::WavPack {
                depth: WavPackDepth::Int16,
            },
            48000,
            2,
        )
        .unwrap();
        writer.write(&[0.5; 100]).unwrap(); // cancellation = drop before finalize
    }
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    let mut writer = WavPackWriter::create(&path, 48000, 1, WavPackDepth::Int24).unwrap();
    assert!(writer.write(&[f32::NAN]).is_err());
    assert!(writer.finalize().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    assert_eq!(std::fs::read_dir(&folder.0).unwrap().count(), 1);
    assert!(is_audio_extension("WV"));
}
