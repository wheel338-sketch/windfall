//! Scanning: the logic with prepared scanner output, then the real scanner
//! program on plugins that load, crash and hang.

mod common;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{ABSURD_PORTS, GAIN, INIT_CRASH, INIT_FAIL, INIT_HANG, SINE, plugin_file, scanner};
use windfall_plugin_host::paths::{PluginFile, find_plugins};
use windfall_plugin_host::scan::{
    FailureKind, FileScan, RunEnd, RunOutput, ScanLine, ScanRequest, ScannerUnavailable,
};
use windfall_plugin_host::{
    AudioPort, PluginCatalog, PluginDescriptor, PluginFormat, PluginKind, PluginLayout, ScanRunner,
    scan_file,
};

fn descriptor(id: &str) -> PluginDescriptor {
    PluginDescriptor {
        format: PluginFormat::Clap,
        id: id.to_owned(),
        name: id.to_uppercase(),
        vendor: "Tests".to_owned(),
        version: "1".to_owned(),
        features: vec!["audio-effect".to_owned()],
        kind: PluginKind::Effect,
    }
}

fn layout() -> PluginLayout {
    let stereo = AudioPort {
        name: "Main".to_owned(),
        channels: 2,
        main: true,
    };
    PluginLayout {
        audio_inputs: vec![stereo.clone()],
        audio_outputs: vec![stereo],
        note_inputs: 0,
        note_outputs: 0,
        has_editor: true,
        parameter_count: 3,
        has_state: true,
    }
}

fn file(ids: &[&str]) -> ScanLine {
    ScanLine::File {
        format: PluginFormat::Clap,
        plugins: ids.iter().map(|id| descriptor(id)).collect(),
    }
}

fn probing(id: &str) -> ScanLine {
    ScanLine::Probing { id: id.to_owned() }
}

fn probed(id: &str) -> ScanLine {
    ScanLine::Probed {
        id: id.to_owned(),
        layout: layout(),
    }
}

/// A scanner that prints what a test tells it to: one prepared run per
/// call, and a record of what it was asked.
struct FakeRunner {
    runs: RefCell<Vec<RunOutput>>,
    requests: RefCell<Vec<(PathBuf, Vec<String>)>>,
}

impl FakeRunner {
    fn new(runs: Vec<RunOutput>) -> Self {
        Self {
            runs: RefCell::new(runs),
            requests: RefCell::new(Vec::new()),
        }
    }

    fn calls(&self) -> usize {
        self.requests.borrow().len()
    }
}

impl ScanRunner for FakeRunner {
    fn run(&self, request: ScanRequest<'_>) -> Result<RunOutput, ScannerUnavailable> {
        self.requests
            .borrow_mut()
            .push((request.path.to_path_buf(), request.skip.to_vec()));
        let mut runs = self.runs.borrow_mut();
        assert!(
            !runs.is_empty(),
            "the scanner was run more often than planned"
        );
        Ok(runs.remove(0))
    }
}

fn finished(mut lines: Vec<ScanLine>) -> RunOutput {
    lines.push(ScanLine::Done);
    RunOutput {
        lines,
        end: RunEnd::Exited(Some(0)),
    }
}

fn crashed(lines: Vec<ScanLine>) -> RunOutput {
    RunOutput {
        lines,
        end: RunEnd::Exited(Some(-1_073_741_819)),
    }
}

fn hung(lines: Vec<ScanLine>) -> RunOutput {
    RunOutput {
        lines,
        end: RunEnd::TimedOut,
    }
}

#[test]
fn the_scanner_output_is_json_lines_and_other_lines_are_ignored() {
    let line = probed("a.b");
    let json = line.to_json();
    assert!(json.starts_with(r#"{"type":"probed","id":"a.b","layout":{"audioInputs":[{"#));
    assert_eq!(ScanLine::parse(&json), Some(line));
    assert_eq!(
        ScanLine::parse("  {\"type\":\"done\"}\r"),
        Some(ScanLine::Done)
    );
    assert_eq!(ScanLine::parse("Loading presets..."), None);
    assert_eq!(ScanLine::parse("{\"type\":\"unknown\"}"), None);
    assert_eq!(ScanLine::parse("{broken"), None);
}

#[test]
fn a_clean_scan_lists_every_plugin_with_its_layout() {
    let runner = FakeRunner::new(vec![finished(vec![
        file(&["one", "two"]),
        probing("one"),
        probed("one"),
        probing("two"),
        probed("two"),
    ])]);
    let scan = scan_file(&runner, Path::new("x.clap")).unwrap();
    assert_eq!(scan.failure, None);
    assert_eq!(scan.plugins.len(), 2);
    assert!(scan.plugins.iter().all(|plugin| plugin.is_usable()));
    assert_eq!(scan.plugins[1].descriptor.id, "two");
    assert_eq!(scan.plugins[1].layout, Some(layout()));
    assert_eq!(runner.calls(), 1);
}

#[test]
fn a_plugin_that_crashes_the_scanner_is_blamed_and_skipped() {
    let runner = FakeRunner::new(vec![
        crashed(vec![
            file(&["one", "bad", "three"]),
            probing("one"),
            probed("one"),
            probing("bad"),
        ]),
        finished(vec![
            file(&["one", "bad", "three"]),
            probing("one"),
            probed("one"),
            probing("three"),
            probed("three"),
        ]),
    ]);
    let scan = scan_file(&runner, Path::new("x.clap")).unwrap();
    assert_eq!(runner.calls(), 2);
    assert_eq!(runner.requests.borrow()[1].1, ["bad"]);

    assert!(scan.plugins[0].is_usable());
    assert!(scan.plugins[2].is_usable());
    let failure = scan.plugins[1].failure.as_ref().unwrap();
    assert_eq!(failure.kind, FailureKind::Crashed);
    assert_eq!(
        windfall_core::english::english(&failure.message),
        "the helper process exited with code 0xc0000005"
    );
    assert!(failure.is_dangerous());
    assert!(scan.plugins[1].layout.is_none());
}

#[test]
fn several_bad_plugins_in_one_file_are_each_found() {
    let ids = ["hang", "good", "crash"];
    let runner = FakeRunner::new(vec![
        hung(vec![file(&ids), probing("hang")]),
        crashed(vec![
            file(&ids),
            probing("good"),
            probed("good"),
            probing("crash"),
        ]),
        finished(vec![file(&ids), probing("good"), probed("good")]),
    ]);
    let scan = scan_file(&runner, Path::new("x.clap")).unwrap();
    assert_eq!(runner.requests.borrow()[2].1, ["hang", "crash"]);
    let kinds: Vec<_> = scan
        .plugins
        .iter()
        .map(|plugin| plugin.failure.as_ref().map(|failure| failure.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            Some(FailureKind::TimedOut),
            None,
            Some(FailureKind::Crashed)
        ]
    );
}

#[test]
fn a_file_that_dies_before_listing_anything_fails_as_a_whole() {
    let runner = FakeRunner::new(vec![crashed(Vec::new())]);
    let scan = scan_file(&runner, Path::new("x.clap")).unwrap();
    assert!(scan.plugins.is_empty());
    assert_eq!(scan.failure.unwrap().kind, FailureKind::Crashed);
    assert_eq!(
        runner.calls(),
        1,
        "there is nothing to skip, so no second run"
    );

    let runner = FakeRunner::new(vec![hung(Vec::new())]);
    let scan = scan_file(&runner, Path::new("x.clap")).unwrap();
    assert_eq!(scan.failure.unwrap().kind, FailureKind::TimedOut);
}

#[test]
fn a_file_or_plugin_that_says_no_is_rejected_and_not_blocked() {
    let runner = FakeRunner::new(vec![finished(vec![ScanLine::Failed {
        message: "not a library".to_owned(),
    }])]);
    let scan = scan_file(&runner, Path::new("x.clap")).unwrap();
    let failure = scan.failure.unwrap();
    assert_eq!(failure.kind, FailureKind::Rejected);
    assert!(!failure.is_dangerous());

    let runner = FakeRunner::new(vec![finished(vec![
        file(&["one"]),
        probing("one"),
        ScanLine::ProbeFailed {
            id: "one".to_owned(),
            message: "it reports 4294967295 audio inputs".to_owned(),
        },
    ])]);
    let scan = scan_file(&runner, Path::new("x.clap")).unwrap();
    assert!(!scan.plugins[0].is_usable());
    assert_eq!(
        scan.plugins[0].failure.as_ref().unwrap().kind,
        FailureKind::Rejected
    );
}

#[test]
fn a_crash_after_the_last_plugin_does_not_cost_the_results() {
    // Some plugins crash while they are unloaded.
    let runner = FakeRunner::new(vec![crashed(vec![
        file(&["one"]),
        probing("one"),
        probed("one"),
    ])]);
    let scan = scan_file(&runner, Path::new("x.clap")).unwrap();
    assert!(scan.plugins[0].is_usable());
    assert_eq!(runner.calls(), 1);
}

#[test]
fn a_scanner_that_keeps_dying_on_the_same_plugin_is_not_run_forever() {
    // A scanner that ignores `--skip` would otherwise loop.
    let stuck = || crashed(vec![file(&["one"]), probing("one")]);
    let runner = FakeRunner::new(vec![stuck(), stuck()]);
    let scan = scan_file(&runner, Path::new("x.clap")).unwrap();
    assert_eq!(runner.calls(), 2);
    assert_eq!(
        scan.plugins[0].failure.as_ref().unwrap().kind,
        FailureKind::Crashed
    );
}

/// Makes a file with some bytes in it and returns it as a plugin file.
fn dummy_plugin(folder: &Path, name: &str, bytes: &[u8]) -> PluginFile {
    std::fs::create_dir_all(folder).unwrap();
    let path = folder.join(name);
    std::fs::write(&path, bytes).unwrap();
    PluginFile {
        path,
        format: PluginFormat::Clap,
    }
}

#[test]
fn the_catalog_scans_only_what_is_new_or_changed() {
    let folder = common::scratch().join("catalog-fresh");
    let one = dummy_plugin(&folder, "one.clap", b"1");
    let two = dummy_plugin(&folder, "two.clap", b"22");
    let files = [one.clone(), two.clone()];
    let scan_of = |id: &str| finished(vec![file(&[id]), probing(id), probed(id)]);

    let mut catalog = PluginCatalog::new();
    let runner = FakeRunner::new(vec![scan_of("one"), scan_of("two")]);
    let mut seen = Vec::new();
    let summary = catalog
        .refresh(&files, &runner, &mut |path| seen.push(path.to_path_buf()))
        .unwrap();
    assert_eq!(
        (summary.scanned, summary.reused, summary.removed),
        (2, 0, 0)
    );
    assert_eq!(seen, [one.path.clone(), two.path.clone()]);
    assert_eq!(catalog.plugins().count(), 2);
    assert!(catalog.find(PluginFormat::Clap, "two").is_some());
    assert!(catalog.find(PluginFormat::Vst3, "two").is_none());

    // Nothing changed: nothing is scanned.
    let runner = FakeRunner::new(Vec::new());
    let summary = catalog.refresh(&files, &runner, &mut |_| {}).unwrap();
    assert_eq!(
        (summary.scanned, summary.reused, summary.removed),
        (0, 2, 0)
    );

    // A file whose size changed is scanned again, and one that is gone is
    // dropped.
    std::fs::write(&one.path, b"a longer file").unwrap();
    let runner = FakeRunner::new(vec![scan_of("one-v2")]);
    let summary = catalog
        .refresh(std::slice::from_ref(&one), &runner, &mut |_| {})
        .unwrap();
    assert_eq!(
        (summary.scanned, summary.reused, summary.removed),
        (1, 0, 1)
    );
    let ids: Vec<_> = catalog
        .plugins()
        .map(|(_, plugin)| plugin.descriptor.id.clone())
        .collect();
    assert_eq!(ids, ["one-v2"]);
}

#[test]
fn the_catalog_survives_being_saved_and_a_damaged_file_starts_empty() {
    let folder = common::scratch().join("catalog-saved");
    let one = dummy_plugin(&folder, "one.clap", b"1");
    let mut catalog = PluginCatalog::new();
    let runner = FakeRunner::new(vec![finished(vec![
        file(&["one"]),
        probing("one"),
        probed("one"),
    ])]);
    catalog.refresh(&[one], &runner, &mut |_| {}).unwrap();

    let stored = folder.join("deeper").join("plugins.json");
    catalog.save(&stored).unwrap();
    catalog.save(&stored).unwrap();
    assert_eq!(PluginCatalog::load(&stored), catalog);
    let left_behind: Vec<_> = std::fs::read_dir(stored.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left_behind, ["plugins.json"], "no temporary file remains");

    std::fs::write(&stored, b"{ half a fi").unwrap();
    assert_eq!(PluginCatalog::load(&stored), PluginCatalog::new());
    std::fs::write(&stored, br#"{"version":999,"files":[]}"#).unwrap();
    assert_eq!(PluginCatalog::load(&stored), PluginCatalog::new());
    assert_eq!(
        PluginCatalog::load(&folder.join("missing.json")),
        PluginCatalog::new()
    );
}

#[test]
fn blocked_plugins_are_not_scanned_again_until_asked() {
    let folder = common::scratch().join("catalog-blocked");
    let bad = dummy_plugin(&folder, "bad.clap", b"bad");
    let dead = dummy_plugin(&folder, "dead.clap", b"dead");
    let files = [bad.clone(), dead.clone()];

    let mut catalog = PluginCatalog::new();
    let runner = FakeRunner::new(vec![
        hung(vec![
            file(&["good", "hangs"]),
            probing("good"),
            probed("good"),
            probing("hangs"),
        ]),
        finished(vec![
            file(&["good", "hangs"]),
            probing("good"),
            probed("good"),
        ]),
        crashed(Vec::new()),
    ]);
    catalog.refresh(&files, &runner, &mut |_| {}).unwrap();

    let blocked = catalog.blocked();
    assert_eq!(blocked.len(), 2);
    assert_eq!(blocked[0].path, bad.path);
    assert_eq!(
        blocked[0].plugin,
        Some(("hangs".to_owned(), "HANGS".to_owned()))
    );
    assert_eq!(blocked[0].failure.kind, FailureKind::TimedOut);
    assert_eq!(blocked[1].path, dead.path);
    assert_eq!(blocked[1].plugin, None);
    assert_eq!(blocked[1].failure.kind, FailureKind::Crashed);
    // The good plugin next to the one that hangs stays usable.
    let usable: Vec<_> = catalog
        .plugins()
        .map(|(_, plugin)| plugin.descriptor.id.as_str())
        .collect();
    assert_eq!(usable, ["good"]);

    // The next start does not touch them. The blocklist is kept on disk.
    let stored = folder.join("plugins.json");
    catalog.save(&stored).unwrap();
    let mut catalog = PluginCatalog::load(&stored);
    let runner = FakeRunner::new(Vec::new());
    let summary = catalog.refresh(&files, &runner, &mut |_| {}).unwrap();
    assert_eq!(summary.reused, 2);
    assert_eq!(catalog.blocked().len(), 2);

    // Retrying one file scans that file again, and it may have been fixed.
    assert!(catalog.retry(&dead.path));
    assert!(!catalog.retry(&dead.path), "it is forgotten already");
    let runner = FakeRunner::new(vec![finished(vec![
        file(&["revived"]),
        probing("revived"),
        probed("revived"),
    ])]);
    let summary = catalog.refresh(&files, &runner, &mut |_| {}).unwrap();
    assert_eq!((summary.scanned, summary.reused), (1, 1));
    assert_eq!(catalog.blocked().len(), 1);

    assert_eq!(catalog.retry_blocked(), 1);
    assert!(catalog.blocked().is_empty());
}

#[test]
fn a_scanner_that_cannot_be_started_is_an_error_and_nothing_is_cached() {
    let folder = common::scratch().join("catalog-no-scanner");
    let one = dummy_plugin(&folder, "one.clap", b"1");
    let mut runner = scanner(Duration::from_secs(5));
    runner.program = folder.join("no-such-scanner.exe");
    let mut catalog = PluginCatalog::new();
    assert!(catalog.refresh(&[one], &runner, &mut |_| {}).is_err());
    assert_eq!(catalog.files().count(), 0);
}

fn real_scan(path: &Path, timeout: Duration) -> FileScan {
    scan_file(&scanner(timeout), path).unwrap()
}

#[test]
fn the_real_scanner_reads_the_test_plugins() {
    let scan = real_scan(&common::test_plugins(), Duration::from_secs(20));
    assert_eq!(scan.failure, None);
    let find = |id: &str| {
        scan.plugins
            .iter()
            .find(|plugin| plugin.descriptor.id == id)
            .unwrap_or_else(|| panic!("{id} was not listed"))
    };

    let gain = find(GAIN);
    assert_eq!(gain.descriptor.name, "Test Gain");
    assert_eq!(gain.descriptor.vendor, "Windfall tests");
    assert_eq!(gain.descriptor.version, "1.2.3");
    assert_eq!(gain.descriptor.kind, PluginKind::Effect);
    assert_eq!(
        gain.descriptor.features,
        ["audio-effect", "utility", "stereo"]
    );
    let layout = gain.layout.as_ref().unwrap();
    assert_eq!(layout.audio_inputs[0].channels, 2);
    assert_eq!(layout.audio_outputs[0].channels, 2);
    assert_eq!(layout.parameter_count, 9);
    assert_eq!(layout.has_editor, cfg!(windows));
    assert!(layout.has_state);

    let sine = find(SINE);
    assert_eq!(sine.descriptor.kind, PluginKind::Instrument);
    let layout = sine.layout.as_ref().unwrap();
    assert!(layout.audio_inputs.is_empty());
    assert_eq!(layout.note_inputs, 1);
    assert!(!layout.has_editor);

    // Plugins that say no are listed with the reason, and their
    // neighbours are unharmed.
    let absurd = find(ABSURD_PORTS);
    let failure = absurd.failure.as_ref().unwrap();
    assert_eq!(failure.kind, FailureKind::Rejected);
    assert!(
        windfall_core::english::english(&failure.message).contains("4294967295 audio inputs"),
        "{failure:?}"
    );
    assert_eq!(
        find(INIT_FAIL).failure.as_ref().unwrap().kind,
        FailureKind::Rejected
    );
    let usable = scan
        .plugins
        .iter()
        .filter(|plugin| plugin.is_usable())
        .count();
    assert_eq!(usable, scan.plugins.len() - 2);
}

#[test]
fn the_real_scanner_survives_a_plugin_that_crashes_on_load() {
    let file = plugin_file("crash", "crash-on-load.clap");
    let scan = real_scan(&file, Duration::from_secs(20));
    assert!(scan.plugins.is_empty());
    let failure = scan.failure.unwrap();
    assert_eq!(failure.kind, FailureKind::Crashed);
    if cfg!(windows) {
        // An access violation.
        assert!(
            windfall_core::english::english(&failure.message).ends_with("0xc0000005"),
            "{failure:?}"
        );
    }
}

#[test]
fn the_real_scanner_gives_up_on_a_plugin_that_hangs_on_load() {
    let file = plugin_file("hang", "hang-on-load.clap");
    let started = std::time::Instant::now();
    let scan = real_scan(&file, Duration::from_secs(2));
    assert_eq!(scan.failure.unwrap().kind, FailureKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(15));
}

#[test]
fn the_real_scanner_isolates_plugins_that_hang_or_crash_in_their_own_init() {
    let file = plugin_file("nasty", "nasty.clap");
    let scan = real_scan(&file, Duration::from_secs(3));
    assert_eq!(scan.failure, None);
    let kind_of = |id: &str| {
        let plugin = scan
            .plugins
            .iter()
            .find(|plugin| plugin.descriptor.id == id)
            .unwrap_or_else(|| panic!("{id} was not listed"));
        plugin.failure.as_ref().map(|failure| failure.kind)
    };
    assert_eq!(kind_of(INIT_HANG), Some(FailureKind::TimedOut));
    assert_eq!(kind_of(INIT_CRASH), Some(FailureKind::Crashed));
    assert_eq!(kind_of(GAIN), None);
    assert_eq!(kind_of(SINE), None);
}

#[test]
fn the_real_scanner_rejects_a_file_that_is_not_a_plugin() {
    let folder = common::scratch().join("junk");
    let junk = dummy_plugin(&folder, "junk.clap", b"this is not a library");
    let scan = real_scan(&junk.path, Duration::from_secs(20));
    assert_eq!(scan.failure.unwrap().kind, FailureKind::Rejected);
}

#[test]
fn a_folder_of_plugins_is_found_scanned_and_cached_end_to_end() {
    let folder = common::scratch().join("end-to-end");
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(folder.join("Vendor")).unwrap();
    std::fs::copy(common::test_plugins(), folder.join("Vendor/tests.clap")).unwrap();
    std::fs::copy(common::test_plugins(), folder.join("crash-on-load.clap")).unwrap();
    std::fs::write(folder.join("readme.txt"), b"not a plugin").unwrap();

    let files = find_plugins(std::slice::from_ref(&folder));
    assert_eq!(files.len(), 2);
    let runner = scanner(Duration::from_secs(20));
    let stored = folder.join("catalog.json");
    let mut catalog = PluginCatalog::load(&stored);
    let summary = catalog.refresh(&files, &runner, &mut |_| {}).unwrap();
    assert_eq!(summary.scanned, 2);
    catalog.save(&stored).unwrap();

    let (path, plugin) = catalog.find(PluginFormat::Clap, GAIN).unwrap();
    assert!(path.ends_with("Vendor/tests.clap"));
    assert_eq!(plugin.descriptor.name, "Test Gain");
    let blocked = catalog.blocked();
    assert_eq!(blocked.len(), 1);
    assert!(blocked[0].path.ends_with("crash-on-load.clap"));

    // A second start reads the file and scans nothing.
    let mut catalog = PluginCatalog::load(&stored);
    let summary = catalog.refresh(&files, &runner, &mut |_| {}).unwrap();
    assert_eq!((summary.scanned, summary.reused), (0, 2));
}

#[test]
fn plugin_chatter_does_not_extend_the_scan_deadline() {
    let path = common::plugin_file("chatty", "chatty-on-load.clap");
    let started = std::time::Instant::now();
    let scan = real_scan(&path, Duration::from_millis(300));
    assert_eq!(scan.failure.unwrap().kind, FailureKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(3));
}
