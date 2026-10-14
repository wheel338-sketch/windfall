use std::{
    fs,
    io::{Cursor, Seek, Write},
    path::Path,
};
use windfall_archive::{self as archive, Error, Limits, Stage};
use windfall_project::{Command, Document, Project, SamplePath, file};
use zip::{ZipWriter, write::SimpleFileOptions};

fn project(root: &Path) -> Project {
    fs::write(root.join("one.wav"), b"same bytes").unwrap();
    fs::write(root.join("two.wav"), b"same bytes").unwrap();
    let mut document = Document::new(Project::new("Archive"));
    for name in ["one.wav", "two.wav"] {
        document
            .dispatch(
                Command::AddSample {
                    name: name.into(),
                    path: SamplePath::Project(name.into()),
                },
                None,
            )
            .unwrap();
    }
    document.project().clone()
}
fn keep_going(_: Stage) -> Result<(), Error> {
    Ok(())
}
fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn empty_project_zip(method: zip::CompressionMethod, streamed: bool) -> Vec<u8> {
    fn write<W: Write + Seek>(mut writer: ZipWriter<W>, method: zip::CompressionMethod) -> W {
        let project = Project::new("Locál 音 project");
        let json = file::to_json(&project).unwrap();
        for (name, bytes) in [
            (
                "manifest.json",
                br#"{"schema":1,"project":"project.windfall","audio":[]}"#.as_slice(),
            ),
            ("project.windfall", json.as_bytes()),
        ] {
            writer
                .start_file(
                    name,
                    SimpleFileOptions::default().compression_method(method),
                )
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap()
    }
    let bytes = if streamed {
        write(ZipWriter::new_stream(Vec::new()), method).into_inner()
    } else {
        write(ZipWriter::new(Cursor::new(Vec::new())), method).into_inner()
    };
    assert!(bytes.len() < 4096);
    bytes
}

fn read_u16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}
fn read_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn central(bytes: &[u8]) -> usize {
    read_u32(bytes, bytes.len() - 22 + 16) as usize
}

fn records(bytes: &[u8]) -> Vec<(usize, usize, usize)> {
    let mut at = central(bytes);
    let count = read_u16(bytes, bytes.len() - 22 + 10);
    (0..count)
        .map(|_| {
            let local = read_u32(bytes, at + 42) as usize;
            let descriptor = local
                + 30
                + read_u16(bytes, local + 26) as usize
                + read_u16(bytes, local + 28) as usize
                + read_u32(bytes, at + 20) as usize;
            let record = (at, local, descriptor);
            at += 46
                + read_u16(bytes, at + 28) as usize
                + read_u16(bytes, at + 30) as usize
                + read_u16(bytes, at + 32) as usize;
            record
        })
        .collect()
}

fn unsigned_descriptors(mut bytes: Vec<u8>) -> Vec<u8> {
    let originals = records(&bytes);
    let directory = central(&bytes) - 4 * originals.len();
    for (_, _, descriptor) in originals.iter().rev() {
        assert_eq!(&bytes[*descriptor..*descriptor + 4], b"PK\x07\x08");
        bytes.drain(*descriptor..*descriptor + 4);
    }
    let end = bytes.len() - 22;
    bytes[end + 16..end + 20].copy_from_slice(&(directory as u32).to_le_bytes());
    let mut at = directory;
    for (_, local, _) in &originals {
        let shift = 4 * originals
            .iter()
            .filter(|(_, _, descriptor)| descriptor < local)
            .count();
        bytes[at + 42..at + 46].copy_from_slice(&((*local - shift) as u32).to_le_bytes());
        at += 46
            + read_u16(&bytes, at + 28) as usize
            + read_u16(&bytes, at + 30) as usize
            + read_u16(&bytes, at + 32) as usize;
    }
    bytes
}

#[test]
fn valid_zip32_descriptors_utf8_flags_and_comments_round_trip() {
    let root = tempfile::tempdir().unwrap();
    for method in [
        zip::CompressionMethod::Stored,
        zip::CompressionMethod::Deflated,
    ] {
        for signature in [false, true] {
            for filled_local in [false, true] {
                let mut bytes = empty_project_zip(method, true);
                if !signature {
                    bytes = unsigned_descriptors(bytes);
                }
                for (at, local, _) in records(&bytes) {
                    if filled_local {
                        let fields: [u8; 12] = bytes[at + 16..at + 28].try_into().unwrap();
                        bytes[local + 14..local + 26].copy_from_slice(&fields);
                    }
                    // Generated member paths remain ASCII; UTF-8 flags and
                    // Unicode project metadata/comments are nevertheless legal.
                    let flags = read_u16(&bytes, at + 8) | 0x800;
                    bytes[at + 8..at + 10].copy_from_slice(&flags.to_le_bytes());
                    bytes[local + 6..local + 8].copy_from_slice(&flags.to_le_bytes());
                }
                let comment = "Archive café 音".as_bytes();
                let end = bytes.len() - 22;
                bytes[end + 20..end + 22].copy_from_slice(&(comment.len() as u16).to_le_bytes());
                bytes.extend_from_slice(comment);
                let source = root.path().join("descriptor.zip");
                fs::write(&source, bytes).unwrap();
                let extracted = archive::extract(
                    &source,
                    &root.path().join("managed"),
                    Limits::default(),
                    &mut keep_going,
                )
                .unwrap();
                assert_eq!(extracted.project, Project::new("Locál 音 project"));
            }
        }
    }
}

#[test]
fn valid_local_extras_may_differ_from_central_extras() {
    let root = tempfile::tempdir().unwrap();
    for method in [
        zip::CompressionMethod::Stored,
        zip::CompressionMethod::Deflated,
    ] {
        let mut bytes = empty_project_zip(method, false);
        let originals = records(&bytes);
        let directory = central(&bytes);
        let extra_length = read_u16(&bytes, 28);
        let extra_at = 30 + read_u16(&bytes, 26) as usize + extra_length as usize;
        let extra = [0xff, 0xff, 2, 0, 1, 2]; // An unknown local-only field.
        bytes[28..30].copy_from_slice(&(extra_length + extra.len() as u16).to_le_bytes());
        bytes.splice(extra_at..extra_at, extra);
        for (at, local, _) in originals {
            let shifted = at + extra.len();
            let offset = local + if local >= extra_at { extra.len() } else { 0 };
            bytes[shifted + 42..shifted + 46].copy_from_slice(&(offset as u32).to_le_bytes());
        }
        let end = bytes.len() - 22;
        bytes[end + 16..end + 20]
            .copy_from_slice(&((directory + extra.len()) as u32).to_le_bytes());
        let source = root.path().join("local-extra.zip");
        fs::write(&source, bytes).unwrap();
        let extracted = archive::extract(
            &source,
            &root.path().join("managed"),
            Limits::default(),
            &mut keep_going,
        )
        .unwrap();
        assert_eq!(extracted.project, Project::new("Locál 音 project"));
    }
}

#[test]
fn local_metadata_and_descriptor_bounds_fail_before_extraction() {
    let root = tempfile::tempdir().unwrap();
    for case in [
        "offset in directory",
        "name length",
        "extra length",
        "ZIP64 local extra",
        "truncated descriptor",
        "unsigned descriptor crc",
    ] {
        let mut bytes = empty_project_zip(zip::CompressionMethod::Deflated, true);
        let at = central(&bytes);
        match case {
            "offset in directory" => {
                bytes[at + 42..at + 46].copy_from_slice(&(at as u32).to_le_bytes())
            }
            "name length" => bytes[26..28].copy_from_slice(&u16::MAX.to_le_bytes()),
            "extra length" => bytes[28..30].copy_from_slice(&u16::MAX.to_le_bytes()),
            "ZIP64 local extra" => {
                // Reinterpret four payload bytes as a local-only ZIP64 extra;
                // no central override is needed to refuse it before payload IO.
                let extra = 30 + read_u16(&bytes, 26) as usize;
                bytes[28..30].copy_from_slice(&4u16.to_le_bytes());
                bytes[extra..extra + 4].copy_from_slice(&[1, 0, 0, 0]);
            }
            "truncated descriptor" => {
                let (_, _, descriptor) = records(&bytes)[1];
                bytes.drain(descriptor + 8..descriptor + 16);
                let end = bytes.len() - 22;
                bytes[end + 16..end + 20].copy_from_slice(&((at - 8) as u32).to_le_bytes());
            }
            "unsigned descriptor crc" => {
                bytes = unsigned_descriptors(bytes);
                let (_, _, descriptor) = records(&bytes)[0];
                bytes[descriptor] ^= 1;
            }
            _ => unreachable!(),
        }
        let source = root.path().join("metadata.zip");
        let managed = root.path().join(case);
        fs::write(&source, bytes).unwrap();
        let mut extracting = false;
        assert!(
            archive::extract(&source, &managed, Limits::default(), &mut |stage| {
                extracting |= stage == Stage::Extract;
                Ok(())
            })
            .is_err(),
            "{case}"
        );
        assert!(!extracting && !managed.exists(), "{case}");
    }
}

#[test]
fn local_records_must_match_the_validated_central_identity() {
    let good = empty_project_zip(zip::CompressionMethod::Deflated, false);
    let root = tempfile::tempdir().unwrap();
    let mut accepted = Vec::new();
    for case in [
        "name",
        "flags",
        "method",
        "crc",
        "compressed size",
        "expanded size",
    ] {
        let mut bytes = good.clone();
        assert_eq!(&bytes[..4], b"PK\x03\x04");
        match case {
            "name" => bytes[30..43].copy_from_slice(b"../ifest.json"),
            "flags" => bytes[6] ^= 1,
            "method" => bytes[8..10].copy_from_slice(&0u16.to_le_bytes()),
            "crc" => bytes[14] ^= 1,
            "compressed size" => bytes[18] ^= 1,
            "expanded size" => bytes[22] ^= 1,
            _ => unreachable!(),
        }
        let source = root.path().join("contradiction.zip");
        let managed = root.path().join(case);
        fs::write(&source, bytes).unwrap();
        let mut extracting = false;
        let result = archive::extract(&source, &managed, Limits::default(), &mut |stage| {
            extracting |= stage == Stage::Extract;
            Ok(())
        });
        if result.is_ok() {
            accepted.push(case);
        } else {
            assert!(
                !extracting && !managed.exists(),
                "{case} was rejected after extraction began"
            );
        }
    }
    assert!(
        accepted.is_empty(),
        "public reader accepted contradictory local records: {accepted:?}"
    );
}

#[test]
fn local_compressed_extents_must_stay_before_the_directory_and_not_overlap() {
    let good = empty_project_zip(zip::CompressionMethod::Deflated, false);
    let root = tempfile::tempdir().unwrap();
    let mut accepted = Vec::new();
    for case in ["beyond EOF", "overlapping next local record"] {
        let mut bytes = good.clone();
        let at = central(&bytes);
        let declared = if case == "beyond EOF" {
            bytes.len() as u32 + 64
        } else {
            read_u32(&bytes, at + 20) + 8
        };
        bytes[18..22].copy_from_slice(&declared.to_le_bytes());
        bytes[at + 20..at + 24].copy_from_slice(&declared.to_le_bytes());
        let source = root.path().join("extent.zip");
        let managed = root.path().join(case);
        fs::write(&source, bytes).unwrap();
        let result = archive::extract(&source, &managed, Limits::default(), &mut keep_going);
        if result.is_ok() {
            accepted.push(case);
        } else {
            assert!(
                !managed.exists(),
                "{case} was rejected after destination creation"
            );
        }
    }
    assert!(
        accepted.is_empty(),
        "public reader accepted invalid compressed extents: {accepted:?}"
    );
}

#[test]
fn local_data_descriptors_must_bind_crc_and_sizes_to_the_directory() {
    let good = empty_project_zip(zip::CompressionMethod::Deflated, true);
    let at = central(&good);
    let data = 30 + read_u16(&good, 26) as usize + read_u16(&good, 28) as usize;
    let descriptor = data + read_u32(&good, at + 20) as usize;
    assert_eq!(&good[descriptor..descriptor + 4], b"PK\x07\x08");
    let root = tempfile::tempdir().unwrap();
    let mut accepted = Vec::new();
    for (case, field) in [
        ("descriptor crc", 4),
        ("descriptor compressed size", 8),
        ("descriptor expanded size", 12),
    ] {
        let mut bytes = good.clone();
        bytes[descriptor + field] ^= 1;
        let source = root.path().join("descriptor.zip");
        let managed = root.path().join(case);
        fs::write(&source, bytes).unwrap();
        if archive::extract(&source, &managed, Limits::default(), &mut keep_going).is_ok() {
            accepted.push(case);
        } else {
            assert!(!managed.exists());
        }
    }
    assert!(
        accepted.is_empty(),
        "public reader accepted contradictory descriptors: {accepted:?}"
    );
}
#[test]
fn exact_dedup_snapshot_and_original_metadata_round_trip() {
    let root = tempfile::tempdir().unwrap();
    let project = project(root.path());
    let target = root.path().join("portable.zip");
    archive::create(
        &project,
        None,
        Some(root.path()),
        root.path(),
        &target,
        Limits::default(),
        &mut keep_going,
    )
    .unwrap();
    fs::remove_file(root.path().join("one.wav")).unwrap();
    fs::remove_file(root.path().join("two.wav")).unwrap();
    let extracted = archive::extract(
        &target,
        &root.path().join("managed"),
        Limits::default(),
        &mut keep_going,
    )
    .unwrap();
    assert_ne!(
        extracted.project.samples[0].path,
        extracted.project.samples[1].path
    );
    let path = file::resolve_sample_path(
        &extracted.project.samples[0].path,
        Some(extracted.root()),
        root.path(),
    )
    .unwrap();
    assert_eq!(fs::read(path).unwrap(), b"same bytes");
    let manifest: archive::Manifest =
        serde_json::from_slice(&fs::read(extracted.root().join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest.audio[0].original_path, project.samples[0].path);
    assert_eq!(manifest.audio[1].original_name, "two.wav");
    assert_eq!(manifest.audio[0].member, manifest.audio[1].member);
    assert_eq!(
        fs::read_dir(extracted.root().join("audio"))
            .unwrap()
            .count(),
        1
    );
    let owned = extracted.root().to_path_buf();
    drop(extracted);
    assert!(!owned.exists());
}
#[test]
fn missing_sources_are_all_reported_without_publishing() {
    let root = tempfile::tempdir().unwrap();
    let project = project(root.path());
    fs::remove_file(root.path().join("one.wav")).unwrap();
    fs::remove_file(root.path().join("two.wav")).unwrap();
    let target = root.path().join("missing.zip");
    let error = archive::create(
        &project,
        None,
        Some(root.path()),
        root.path(),
        &target,
        Limits::default(),
        &mut keep_going,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("one.wav") && error.contains("two.wav"));
    assert!(!target.exists());
}
#[test]
fn cancellation_and_injected_failures_clean_only_request_staging() {
    for stage in [Stage::Source, Stage::Write, Stage::Publish] {
        for cancellation in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let project = project(root.path());
            let target = root.path().join("portable.zip");
            fs::write(root.path().join("unrelated"), b"keep").unwrap();
            let mut check = |at| {
                if at != stage {
                    Ok(())
                } else if cancellation {
                    Err(Error::Cancelled)
                } else {
                    Err(Error::Io(std::io::Error::other("injected IO failure")))
                }
            };
            assert!(
                archive::create(
                    &project,
                    None,
                    Some(root.path()),
                    root.path(),
                    &target,
                    Limits::default(),
                    &mut check
                )
                .is_err()
            );
            assert!(!target.exists());
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 3);
        }
    }
    for stage in [Stage::Read, Stage::Extract, Stage::Ready] {
        let root = tempfile::tempdir().unwrap();
        let project = project(root.path());
        let target = root.path().join("portable.zip");
        archive::create(
            &project,
            None,
            Some(root.path()),
            root.path(),
            &target,
            Limits::default(),
            &mut keep_going,
        )
        .unwrap();
        let managed = root.path().join("managed");
        fs::create_dir(&managed).unwrap();
        fs::write(managed.join("unrelated"), b"keep").unwrap();
        for cancellation in [false, true] {
            let mut check = |at| {
                if at != stage {
                    Ok(())
                } else if cancellation {
                    Err(Error::Cancelled)
                } else {
                    Err(Error::Io(std::io::Error::other(
                        "injected read/write failure",
                    )))
                }
            };
            assert!(archive::extract(&target, &managed, Limits::default(), &mut check).is_err());
            assert_eq!(fs::read_dir(&managed).unwrap().count(), 1);
        }
    }
}
#[test]
fn competing_target_is_never_replaced() {
    let root = tempfile::tempdir().unwrap();
    let project = project(root.path());
    let target = root.path().join("portable.zip");
    let mut check = |stage| {
        if stage == Stage::Publish {
            fs::write(&target, b"competitor")?;
        }
        Ok(())
    };
    assert!(
        archive::create(
            &project,
            None,
            Some(root.path()),
            root.path(),
            &target,
            Limits::default(),
            &mut check
        )
        .is_err()
    );
    assert_eq!(fs::read(target).unwrap(), b"competitor");
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 3);
}
#[test]
fn source_change_during_staging_refuses_publication() {
    let root = tempfile::tempdir().unwrap();
    let project = project(root.path());
    let target = root.path().join("portable.zip");
    let mut changed = false;
    let mut check = |stage| {
        if stage == Stage::Source && !changed {
            changed = true;
            // The source is already open and its initial size captured.
            // A writer adds bytes before the first bounded read.
            fs::OpenOptions::new()
                .append(true)
                .open(root.path().join("one.wav"))?
                .write_all(b" changed")?;
        }
        Ok(())
    };
    let error = archive::create(
        &project,
        None,
        Some(root.path()),
        root.path(),
        &target,
        Limits::default(),
        &mut check,
    )
    .unwrap_err();
    assert!(
        windfall_core::english::english(&error.to_string()).contains("changed while packaging")
    );
    assert!(!target.exists());
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
}
#[test]
fn unsafe_zip_names_case_collisions_types_and_limits_are_rejected_before_destination_creation() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("bad.zip");
    let managed = root.path().join("managed");
    for name in [
        "../escape",
        "/absolute",
        "C:/drive",
        "//host/share",
        "a\\b",
        "NUL.wav",
        "audio/trailing.",
        "audio/a:b",
        "audio/../escape",
        "Audio/0001.wav",
        "audio/",
        "audio/音.wav",
    ] {
        fs::write(&source, zip(&[(name, b"x")])).unwrap();
        assert!(
            archive::extract(&source, &managed, Limits::default(), &mut keep_going).is_err(),
            "{name}"
        );
        assert!(!managed.exists());
    }
    fs::write(&source, zip(&[("audio/a", b"x"), ("audio/A", b"y")])).unwrap();
    assert!(archive::extract(&source, &managed, Limits::default(), &mut keep_going).is_err());
    // Central-directory mode marks a symbolic link, independent of its content.
    let mut bytes = zip(&[("audio/link", b"../escape")]);
    let at = bytes.windows(4).position(|s| s == b"PK\x01\x02").unwrap();
    bytes[at + 38..at + 42].copy_from_slice(&(0o120777u32 << 16).to_le_bytes());
    fs::write(&source, bytes).unwrap();
    assert!(archive::extract(&source, &managed, Limits::default(), &mut keep_going).is_err());
    fs::write(&source, zip(&[("large", &[0; 256])])).unwrap();
    for limits in [
        Limits {
            entries: 0,
            ..Limits::default()
        },
        Limits {
            entry_bytes: 128,
            ..Limits::default()
        },
        Limits {
            expanded_bytes: 128,
            ..Limits::default()
        },
        Limits {
            archive_bytes: 1,
            ..Limits::default()
        },
    ] {
        assert!(archive::extract(&source, &managed, limits, &mut keep_going).is_err());
        assert!(!managed.exists());
    }
}
#[test]
fn highly_compressed_audio_is_bounded_before_destination_creation() {
    let root = tempfile::tempdir().unwrap();
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            "audio/0001.wav",
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
    writer.write_all(&vec![0; 1024 * 1024]).unwrap();
    let zip = writer.finish().unwrap().into_inner();
    assert!(zip.len() < 2048);
    let source = root.path().join("bomb.zip");
    fs::write(&source, zip).unwrap();
    let managed = root.path().join("managed");
    let limits = Limits {
        entry_bytes: 512 * 1024,
        ..Limits::default()
    };
    let error = archive::extract(&source, &managed, limits, &mut keep_going)
        .err()
        .unwrap();
    assert!(error.to_string().contains("size limit"));
    assert!(!managed.exists());
}
#[test]
fn schema_missing_members_external_paths_crc_and_truncation_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("bad.zip");
    let managed = root.path().join("managed");
    let project = project(root.path());
    let json = file::to_json(&project).unwrap();
    for manifest in [
        br#"{"schema":99,"project":"project.windfall","audio":[]}"#.as_slice(),
        br#"{"schema":1,"project":"project.windfall","audio":[]}"#.as_slice(),
    ] {
        fs::write(
            &source,
            zip(&[
                ("manifest.json", manifest),
                ("project.windfall", json.as_bytes()),
            ]),
        )
        .unwrap();
        assert!(archive::extract(&source, &managed, Limits::default(), &mut keep_going).is_err());
        assert!(!managed.exists());
    }
    archive::create(
        &project,
        None,
        Some(root.path()),
        root.path(),
        &source.with_extension("good.zip"),
        Limits::default(),
        &mut keep_going,
    )
    .unwrap();
    let good = fs::read(source.with_extension("good.zip")).unwrap();
    fs::write(&source, &good[..good.len() - 10]).unwrap();
    assert!(archive::extract(&source, &managed, Limits::default(), &mut keep_going).is_err());
    let mut damaged = good;
    let at = damaged.windows(4).position(|s| s == b"PK\x01\x02").unwrap();
    // Bind the same incorrect CRC in both records so preflight succeeds and
    // the streamed payload reader must still detect the CRC disagreement.
    damaged[14] ^= 1;
    damaged[at + 16] ^= 1;
    fs::write(&source, damaged).unwrap();
    assert!(archive::extract(&source, &managed, Limits::default(), &mut keep_going).is_err());
}

#[test]
fn ordinary_archive_comment_round_trips_through_the_validated_directory() {
    let root = tempfile::tempdir().unwrap();
    let project = Project::new("Commented archive");
    let json = file::to_json(&project).unwrap();
    let mut bytes = zip(&[
        (
            "manifest.json",
            br#"{"schema":1,"project":"project.windfall","audio":[]}"#,
        ),
        ("project.windfall", json.as_bytes()),
    ]);
    let comment = b"ordinary UTF-8 archive comment";
    let end = bytes.len() - 22;
    bytes[end + 20..end + 22].copy_from_slice(&(comment.len() as u16).to_le_bytes());
    bytes.extend_from_slice(comment);
    let source = root.path().join("commented.zip");
    fs::write(&source, bytes).unwrap();
    let extracted = archive::extract(
        &source,
        &root.path().join("managed"),
        Limits::default(),
        &mut keep_going,
    )
    .unwrap();
    assert_eq!(extracted.project, project);
}
