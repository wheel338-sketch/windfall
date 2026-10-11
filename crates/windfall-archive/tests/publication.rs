//! Local disk and real process-death tests of the production archive publisher.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use windfall_archive::{self as archive, Error, Limits, Stage};
use windfall_project::{Command, Document, Project, SamplePath, file};
fn large_project(root: &Path) -> Project {
    let mut doc = Document::new(Project::new("Large publication"));
    // 128 independently named one-MiB files; streamed generation keeps test
    // memory bounded and prevents deduplication concealing copy work.
    for index in 0..128_u32 {
        let path = root.join(format!("source-{index}.raw"));
        let mut output = fs::File::create(&path).unwrap();
        for block in 0..16_u32 {
            let bytes: Vec<_> = (0..65536_u32)
                .map(|i| ((i.wrapping_mul(31) ^ index.wrapping_mul(17) ^ block) & 255) as u8)
                .collect();
            output.write_all(&bytes).unwrap();
        }
        doc.dispatch(
            Command::AddSample {
                name: format!("Source {index}"),
                path: SamplePath::External(path.to_string_lossy().into_owned()),
            },
            None,
        )
        .unwrap();
    }
    doc.project().clone()
}
fn okay(_: Stage) -> Result<(), Error> {
    Ok(())
}
#[test]
fn publication_child() {
    let Some(root) = std::env::var_os("WINDFALL_ARCHIVE_FAULT_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let stage = std::env::var("WINDFALL_ARCHIVE_FAULT_STAGE").unwrap();
    let project = file::load(root.join("input.windfall")).unwrap();
    let mut hits = 0;
    let _ = archive::create(
        &project,
        None,
        None,
        &root,
        &root.join("result.zip"),
        Limits::default(),
        &mut |boundary| {
            if format!("{boundary:?}") == stage {
                hits += 1;
                if !matches!(boundary, Stage::Source | Stage::Write) || hits == 3 {
                    std::process::exit(73);
                }
            }
            Ok(())
        },
    );
    panic!("requested interruption boundary was not reached");
}
#[test]
fn large_archive_is_whole_or_absent_after_real_process_death_and_can_be_retried() {
    let root = tempfile::tempdir().unwrap();
    let project = large_project(root.path());
    file::save(&project, root.path().join("input.windfall")).unwrap();
    let old = root.path().join("older.zip");
    fs::write(&old, b"older archive untouched").unwrap();
    for stage in ["Reservation", "Source", "Write", "Publish", "Published"] {
        let target = root.path().join("result.zip");
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "publication_child", "--nocapture"])
            .env("WINDFALL_ARCHIVE_FAULT_ROOT", root.path())
            .env("WINDFALL_ARCHIVE_FAULT_STAGE", stage)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(73), "{stage}");
        assert_eq!(fs::read(&old).unwrap(), b"older archive untouched");
        if stage == "Published" {
            let extracted = archive::extract(
                &target,
                &root.path().join("managed"),
                Limits::default(),
                &mut okay,
            )
            .unwrap();
            assert_eq!(extracted.project.samples.len(), 128);
            for (sample, original) in extracted.project.samples.iter().zip(&project.samples) {
                let SamplePath::Project(path) = &sample.path else {
                    panic!("portable path required")
                };
                let SamplePath::External(source) = &original.path else {
                    unreachable!()
                };
                assert_eq!(
                    fs::read(extracted.root().join(path)).unwrap(),
                    fs::read(source).unwrap()
                );
            }
            fs::remove_file(&target).unwrap();
        } else {
            assert!(!target.exists(), "{stage}");
        }
        let retry = root.path().join(format!("retry-{stage}.zip"));
        archive::create(
            &project,
            None,
            None,
            root.path(),
            &retry,
            Limits::default(),
            &mut okay,
        )
        .unwrap();
        let before = fs::read(&retry).unwrap();
        assert!(
            archive::create(
                &project,
                None,
                None,
                root.path(),
                &retry,
                Limits::default(),
                &mut okay
            )
            .is_err()
        );
        assert_eq!(fs::read(&retry).unwrap(), before);
    }
}
#[test]
fn errors_at_reservation_copy_write_and_rename_boundaries_clean_request_staging() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("audio.raw");
    fs::write(&source, vec![9; 256 * 1024]).unwrap();
    let mut doc = Document::new(Project::new("Fault"));
    doc.dispatch(
        Command::AddSample {
            name: "Source".into(),
            path: SamplePath::External(source.to_string_lossy().into_owned()),
        },
        None,
    )
    .unwrap();
    for failure in [
        Stage::Reservation,
        Stage::Source,
        Stage::Write,
        Stage::Publish,
    ] {
        let target = root.path().join("result.zip");
        let count = fs::read_dir(root.path()).unwrap().count();
        let mut hits = 0;
        let result = archive::create(
            doc.project(),
            None,
            None,
            root.path(),
            &target,
            Limits::default(),
            &mut |stage| {
                if stage == failure {
                    hits += 1;
                    if !matches!(stage, Stage::Source | Stage::Write) || hits == 2 {
                        return Err(Error::Io(std::io::Error::other("injected disk failure")));
                    }
                }
                Ok(())
            },
        );
        assert!(result.unwrap_err().to_string().contains("injected"));
        assert!(!target.exists());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), count);
        assert_eq!(fs::read(&source).unwrap(), vec![9; 256 * 1024]);
    }
}
