use windfall_codec::{CodecError, DecodeOptions, WavSampleFormat, WavWriter, decode_file_blocks};
#[path = "it/heap.rs"]
mod heap;
#[global_allocator]
static ALLOCATOR: heap::Metered = heap::Metered;

struct Directory(std::path::PathBuf);
impl Directory {
    fn new(label: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("windfall-streamed-{}-{label}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn long_strict_decode_emits_exact_bounded_pcm_and_refuses_quota_or_cancellation() {
    let root = Directory::new("long");
    let path = root.path().join("long.wav");
    let mut writer = WavWriter::create(&path, 16000, 2, WavSampleFormat::Float32).unwrap();
    let mut block = [0.0_f32; 4096];
    for frame in block.as_chunks_mut::<2>().0 {
        frame.copy_from_slice(&[0.125, -0.25]);
    }
    let count = 16000 * 150 * 2;
    let mut remaining = count;
    while remaining > 0 {
        let n = remaining.min(block.len());
        writer.write(&block[..n]).unwrap();
        remaining -= n;
    }
    writer.finalize().unwrap();
    let mut emitted = 0_usize;
    let (result, peak) = heap::peak_heap(|| {
        decode_file_blocks::<CodecError>(
            &path,
            &DecodeOptions {
                max_decoded_bytes: count as u64 * 4,
            },
            |rate, channels, samples| {
                assert_eq!((rate, channels), (16000, 2));
                assert!(samples.len() <= 4096 && samples.len().is_multiple_of(2));
                assert!(
                    samples
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .all(|frame| *frame == [0.125, -0.25])
                );
                emitted += samples.len();
                Ok(())
            },
        )
        .unwrap()
    });
    assert!(
        peak <= 2 * 1024 * 1024,
        "stream verification heap peak={peak}"
    );
    assert_eq!(result, (16000, 2, 16000 * 150));
    assert_eq!(emitted, count);
    assert!(matches!(
        decode_file_blocks::<CodecError>(
            &path,
            &DecodeOptions {
                max_decoded_bytes: count as u64 * 4 - 1
            },
            |_, _, _| Ok(())
        ),
        Err(CodecError::TooLarge { .. })
    ));
    let mut calls = 0;
    assert!(matches!(
        decode_file_blocks::<CodecError>(&path, &DecodeOptions::default(), |_, _, _| {
            calls += 1;
            Err(CodecError::Corrupt("cancelled by caller".into()))
        }),
        Err(CodecError::Corrupt(_))
    ));
    assert_eq!(calls, 1);
    let mut calls = 0;
    assert!(matches!(
        decode_file_blocks::<CodecError>(&path, &DecodeOptions::default(), |_, _, _| {
            calls += 1;
            panic!("authored streaming callback failure");
        }),
        Err(CodecError::Corrupt(_))
    ));
    assert_eq!(calls, 1, "a failed callback must never be replayed");
}

#[test]
fn streamed_float_verification_refuses_nonfinite_and_truncated_output() {
    let root = Directory::new("invalid");
    let path = root.path().join("invalid.wav");
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut bytes = Vec::new();
        bytes.extend(b"RIFF");
        bytes.extend(40_u32.to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16_u32.to_le_bytes());
        bytes.extend(3_u16.to_le_bytes());
        bytes.extend(1_u16.to_le_bytes());
        bytes.extend(8000_u32.to_le_bytes());
        bytes.extend(32000_u32.to_le_bytes());
        bytes.extend(4_u16.to_le_bytes());
        bytes.extend(32_u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend(4_u32.to_le_bytes());
        bytes.extend(value.to_le_bytes());
        std::fs::write(&path, bytes).unwrap();
        assert!(matches!(
            decode_file_blocks::<CodecError>(&path, &DecodeOptions::default(), |_, _, _| Ok(())),
            Err(CodecError::Corrupt(_))
        ));
    }
    let mut truncated = std::fs::read(&path).unwrap();
    truncated.truncate(44);
    std::fs::write(&path, truncated).unwrap();
    assert!(
        decode_file_blocks::<CodecError>(&path, &DecodeOptions::default(), |_, _, _| Ok(()))
            .is_err()
    );
}
