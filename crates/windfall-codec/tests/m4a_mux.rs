//! Structural mux tests use invented payload bytes, not valid AAC audio.
//! Actual independent decode/playback acceptance awaits an approved encoder.
use windfall_codec::*;
fn be(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(b[at..at + 4].try_into().unwrap())
}
fn boxes(b: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut result = Vec::new();
    let mut at = 0;
    while at < b.len() {
        let n = be(b, at) as usize;
        assert!(n >= 8 && at + n <= b.len());
        result.push((b[at + 4..at + 8].try_into().unwrap(), &b[at + 8..at + n]));
        at += n;
    }
    assert_eq!(at, b.len());
    result
}
fn child<'a>(b: &'a [u8], id: &[u8; 4]) -> &'a [u8] {
    boxes(b).into_iter().find(|(tag, _)| tag == id).unwrap().1
}
#[test]
fn mux_tables_offsets_duration_trim_and_tags_match_independent_box_walker() {
    let path = std::env::temp_dir().join(format!("windfall-m4a-mux-{}.m4a", std::process::id()));
    let config = AacConfig {
        sample_rate: 48000,
        channels: 2,
    };
    let mut writer = M4aWriter::create(
        &path,
        config,
        &AudioTags {
            title: "Own fixture",
            author: "Original author",
            ..Default::default()
        },
    )
    .unwrap();
    writer.write_packet(&[0x21, 1, 2]).unwrap();
    writer.write_packet(&[0x21, 3, 4, 5, 6]).unwrap();
    writer
        .finalize(AacTrim {
            delay: 64,
            padding: 32,
        })
        .unwrap();
    let b = std::fs::read(&path).unwrap();
    let inspected = inspect_m4a(&b).unwrap();
    assert_eq!(inspected.config, config);
    assert_eq!(
        inspected.trim,
        AacTrim {
            delay: 64,
            padding: 32
        }
    );
    assert_eq!(inspected.frames(), 1952);
    assert_eq!(
        inspected
            .packet_ranges
            .iter()
            .map(|r| &b[r.clone()])
            .collect::<Vec<_>>(),
        [&[0x21, 1, 2][..], &[0x21, 3, 4, 5, 6][..]]
    );
    assert_eq!(&child(&b, b"ftyp")[..4], b"M4A ");
    assert_eq!(child(&b, b"mdat"), &[0x21, 1, 2, 0x21, 3, 4, 5, 6]);
    let moov = child(&b, b"moov");
    let mv = child(moov, b"mvhd");
    assert_eq!(be(mv, 12), 48000);
    assert_eq!(be(mv, 16), 1952);
    let trak = child(moov, b"trak");
    let edit = child(child(trak, b"edts"), b"elst");
    assert_eq!(be(edit, 8), 1952);
    assert_eq!(be(edit, 12), 64);
    let mdia = child(trak, b"mdia");
    let md = child(mdia, b"mdhd");
    assert_eq!(be(md, 12), 48000);
    assert_eq!(be(md, 16), 2048);
    let stbl = child(child(mdia, b"minf"), b"stbl");
    let stts = child(stbl, b"stts");
    assert_eq!((be(stts, 4), be(stts, 8), be(stts, 12)), (1, 2, 1024));
    let sizes = child(stbl, b"stsz");
    assert_eq!(
        (be(sizes, 4), be(sizes, 8), be(sizes, 12), be(sizes, 16)),
        (0, 2, 3, 5)
    );
    let stco = child(stbl, b"stco");
    let at = be(stco, 8) as usize;
    assert_eq!(&b[at..at + 8], child(&b, b"mdat"));
    let stsc = child(stbl, b"stsc");
    assert_eq!((be(stsc, 8), be(stsc, 12), be(stsc, 16)), (1, 2, 1));
    let stsd = child(stbl, b"stsd");
    let entry = child(&stsd[8..], b"mp4a");
    assert_eq!(be(entry, 24), 48000 << 16);
    let esds = child(&entry[28..], b"esds");
    assert!(esds.windows(4).any(|x| x == [5, 2, 0x11, 0x90])); // AAC-LC 48k stereo ASC
    let meta = child(child(moov, b"udta"), b"meta");
    let ilst = child(&meta[4..], b"ilst");
    let title = child(child(ilst, b"\xa9nam"), b"data");
    assert_eq!(be(title, 0), 1);
    assert_eq!(&title[8..], b"Own fixture");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn mp4_parser_refuses_truncated_boxes_and_hostile_sample_tables() {
    let path = std::env::temp_dir().join(format!("windfall-m4a-parser-{}.m4a", std::process::id()));
    let mut writer = M4aWriter::create(
        &path,
        AacConfig {
            sample_rate: 48000,
            channels: 2,
        },
        &Default::default(),
    )
    .unwrap();
    writer.write_packet(&[0x21, 1, 2]).unwrap();
    writer.finalize(Default::default()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    for n in 0..bytes.len() {
        assert!(
            std::panic::catch_unwind(|| inspect_m4a(&bytes[..n]))
                .unwrap()
                .is_err(),
            "truncated at {n}"
        );
    }
    for tag in [b"stsz", b"stco", b"elst", b"stts"] {
        let at = bytes.windows(4).position(|b| b == tag).unwrap();
        let mut attack = bytes.clone();
        attack[at + 12..at + 16].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(
            std::panic::catch_unwind(|| inspect_m4a(&attack))
                .unwrap()
                .is_err()
        );
    }
}
#[test]
fn refusal_and_abandoned_mux_do_not_replace_existing_destination() {
    let path =
        std::env::temp_dir().join(format!("windfall-m4a-refusal-{}.m4a", std::process::id()));
    std::fs::write(&path, b"old").unwrap();
    let config = AacConfig {
        sample_rate: 44100,
        channels: 1,
    };
    assert!(
        M4aWriter::create(
            &path,
            AacConfig {
                sample_rate: 96000,
                ..config
            },
            &AudioTags::default()
        )
        .is_err()
    );
    let mut writer = M4aWriter::create(&path, config, &AudioTags::default()).unwrap();
    assert!(writer.write_packet(&[]).is_err());
    assert!(writer.write_packet(&[0xff, 0xf1, 0]).is_err());
    assert!(
        writer
            .write_packet(&vec![0; MAX_AAC_PACKET_BYTES + 1])
            .is_err()
    );
    writer.write_packet(&[0x21, 0]).unwrap();
    assert!(
        writer
            .finalize(AacTrim {
                delay: 1024,
                padding: 0
            })
            .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"old");
    let writer = M4aWriter::create(&path, config, &AudioTags::default()).unwrap();
    drop(writer);
    assert_eq!(std::fs::read(&path).unwrap(), b"old");
    std::fs::remove_file(path).unwrap();
}
