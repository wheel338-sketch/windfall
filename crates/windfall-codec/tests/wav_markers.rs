//! Independent byte inspection of original generated metadata fixtures.
use windfall_codec::*;
fn le(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}
fn chunks(b: &[u8]) -> Vec<([u8; 4], &[u8])> {
    assert_eq!(&b[..4], b"RIFF");
    assert_eq!(le(b, 4) as usize + 8, b.len());
    let mut out = Vec::new();
    let mut pos = 12;
    while pos < b.len() {
        let n = le(b, pos + 4) as usize;
        out.push((
            b[pos..pos + 4].try_into().unwrap(),
            &b[pos + 8..pos + 8 + n],
        ));
        pos += 8 + n + n % 2;
    }
    assert_eq!(pos, b.len());
    out
}
fn metadata() -> WavMetadata {
    WavMetadata {
        cues: vec![
            WavCue {
                id: 7,
                frame: 2,
                label: "AB".into(),
            },
            WavCue {
                id: 8,
                frame: 12,
                label: "Slice".into(),
            },
        ],
        loops: vec![WavLoop {
            cue_id: 7,
            start: 2,
            end: 12,
            kind: WavLoopKind::Alternating,
            play_count: 3,
        }],
        slices: vec![WavSlice {
            cue_id: 8,
            length: 4,
        }],
        tempo: Some(WavTempo {
            bpm: 123.5,
            beats: 4,
            numerator: 3,
            denominator: 4,
            root_key: Some(57),
            one_shot: false,
        }),
        root_key: Some(57),
    }
}
#[test]
fn cue_labels_slices_loops_and_tempo_have_independent_expected_bytes() {
    let path =
        std::env::temp_dir().join(format!("windfall-marker-bytes-{}.wav", std::process::id()));
    let expected = metadata();
    let mut writer = WavWriter::create_with_metadata(
        &path,
        48000,
        2,
        WavSampleFormat::Float32,
        &AudioTags::default(),
        &expected,
    )
    .unwrap();
    writer.write(&[0.0; 64]).unwrap();
    writer.finalize().unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let table = chunks(&bytes);
    let cue = table.iter().find(|(id, _)| id == b"cue ").unwrap().1;
    assert_eq!(cue.len(), 52);
    assert_eq!(le(cue, 0), 2);
    assert_eq!((le(cue, 4), le(cue, 8), le(cue, 24)), (7, 2, 2));
    assert_eq!(&cue[12..16], b"data");
    let list = table
        .iter()
        .find(|(id, b)| id == b"LIST" && b.starts_with(b"adtl"))
        .unwrap()
        .1;
    assert_eq!(&list[4..8], b"labl");
    assert_eq!(le(list, 8), 7);
    assert_eq!(&list[12..20], &[7, 0, 0, 0, b'A', b'B', 0, 0]); // size excludes final pad
    let region = list.windows(4).position(|b| b == b"ltxt").unwrap();
    assert_eq!(le(list, region + 4), 20);
    assert_eq!(le(list, region + 8), 8);
    assert_eq!(le(list, region + 12), 4);
    assert_eq!(&list[region + 16..region + 20], b"rgn ");
    let smpl = table.iter().find(|(id, _)| id == b"smpl").unwrap().1;
    assert_eq!(le(smpl, 8), 20833);
    assert_eq!(le(smpl, 12), 57);
    assert_eq!(le(smpl, 28), 1);
    assert_eq!(
        (
            le(smpl, 36),
            le(smpl, 40),
            le(smpl, 44),
            le(smpl, 48),
            le(smpl, 56)
        ),
        (7, 1, 2, 11, 3)
    );
    let acid = table.iter().find(|(id, _)| id == b"acid").unwrap().1;
    assert_eq!(acid.len(), 24);
    assert_eq!(le(acid, 0), 2);
    assert_eq!(&acid[16..20], &[4, 0, 3, 0]);
    assert_eq!(le(acid, 20), 123.5_f32.to_bits());
    assert_eq!(read_wav_metadata(&bytes).unwrap(), expected);
    assert_eq!(read_wav_metadata_file(&path).unwrap(), expected);
    let audio = decode_file(&path).unwrap();
    assert_eq!(audio.frames(), 32);
    std::fs::remove_file(path).unwrap();
}
fn fixture(parts: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut bytes = b"RIFF\0\0\0\0WAVE".to_vec();
    for (id, body) in parts {
        bytes.extend_from_slice(*id);
        bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
        bytes.extend(body);
        if body.len() % 2 != 0 {
            bytes.push(0);
        }
    }
    let size = bytes.len() as u32 - 8;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    bytes
}
fn fmt() -> Vec<u8> {
    vec![1, 0, 1, 0, 0x80, 0xbb, 0, 0, 0, 0x77, 1, 0, 2, 0, 16, 0]
}
#[test]
fn reader_accepts_hand_constructed_cue_after_data_and_skips_unknown_chunks() {
    let mut cue = 1_u32.to_le_bytes().to_vec();
    for v in [9_u32, 1] {
        cue.extend_from_slice(&v.to_le_bytes());
    }
    cue.extend_from_slice(b"data");
    for v in [0_u32, 0, 1] {
        cue.extend_from_slice(&v.to_le_bytes());
    }
    let bytes = fixture(&[
        (b"fmt ", fmt()),
        (b"JUNK", vec![1, 2, 3]),
        (b"data", vec![0; 8]),
        (b"cue ", cue),
    ]);
    assert_eq!(
        read_wav_metadata(&bytes).unwrap().cues,
        [WavCue {
            id: 9,
            frame: 1,
            label: String::new()
        }]
    );
}
#[test]
fn hostile_counts_truncated_chunks_duplicate_ids_and_beyond_audio_are_refused() {
    let bytes = fixture(&[
        (b"fmt ", fmt()),
        (b"data", vec![0; 8]),
        (b"cue ", u32::MAX.to_le_bytes().to_vec()),
    ]);
    assert!(read_wav_metadata(&bytes).is_err());
    for n in 0..bytes.len() {
        assert!(read_wav_metadata(&bytes[..n]).is_err());
    }
    let mut duplicate = metadata();
    duplicate.cues[1].id = 7;
    assert!(duplicate.validate(None).is_err());
    assert!(metadata().validate(Some(5)).is_err());
    let mut nan = metadata();
    nan.tempo.as_mut().unwrap().bpm = f32::NAN;
    assert!(nan.validate(None).is_err());
    let bytes = fixture(&[
        (b"fmt ", fmt()),
        (b"data", vec![0; 8]),
        (b"LIST", vec![0; MAX_WAV_METADATA_BYTES + 1]),
    ]);
    assert!(read_wav_metadata(&bytes).is_err());
}
#[test]
fn failed_finalization_preserves_destination_and_drop_removes_staging() {
    let path = std::env::temp_dir().join(format!(
        "windfall-marker-refusal-{}.wav",
        std::process::id()
    ));
    std::fs::write(&path, b"original").unwrap();
    let writer = WavWriter::create_with_metadata(
        &path,
        48000,
        1,
        WavSampleFormat::Int16,
        &AudioTags::default(),
        &metadata(),
    )
    .unwrap();
    assert!(writer.finalize().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    std::fs::remove_file(path).unwrap();
}
