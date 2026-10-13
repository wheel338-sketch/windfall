//! Native macOS CFBundle acceptance. Other platforms exercise the exact pure
//! lifecycle via unit tests; this suite requires real CoreFoundation/clang.
#![cfg(target_os = "macos")]
mod common;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::process::Stdio;
use std::time::{Duration, Instant};
use windfall_plugin_host::{PluginHost, paths};

fn probe(name: &str, variant: Option<&str>) -> (PathBuf, PathBuf) {
    probe_with_callback(name, variant, None)
}
fn probe_with_callback(
    name: &str,
    variant: Option<&str>,
    callback: Option<extern "C" fn()>,
) -> (PathBuf, PathBuf) {
    let folder = common::scratch().join(name);
    std::fs::create_dir_all(&folder).unwrap();
    let ledger = folder.join("ledger.txt");
    let library = folder.join("probe.dylib");
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vst3_bundle_lifecycle.cpp");
    let mut command = Command::new("xcrun");
    command
        .args([
            "clang++",
            "-std=c++17",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-dynamiclib",
            "-fvisibility=hidden",
            "-framework",
            "CoreFoundation",
        ])
        .arg(format!("-DLEDGER={:?}", ledger.to_str().unwrap()));
    if let Some(variant) = variant {
        command.arg(format!("-D{variant}"));
    }
    if let Some(callback) = callback {
        command.arg(format!("-DREENTRY_CALLBACK={}", callback as usize));
    }
    let log = folder.join("compiler.log");
    let mut child = command
        .arg(source)
        .arg("-o")
        .arg(&library)
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("native probe compilation exceeded 30 seconds");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(
        status.success(),
        "native probe/header ABI compilation failed: {}",
        std::fs::read_to_string(log).unwrap()
    );
    let bundle = folder.join("not-the-executable.vst3");
    common::macos_bundle::publish(&bundle, &library, "native-ledger");
    (bundle, ledger)
}
fn events(ledger: &Path) -> String {
    // U is only an observation; final CF release does not promise unmapping.
    std::fs::read_to_string(ledger)
        .unwrap_or_default()
        .replace('U', "")
}

#[test]
fn mandatory_exports_are_verified_before_any_entry_attempt() {
    let host = PluginHost::windfall();
    for (name, define) in [
        ("missing-entry", "OMIT_ENTRY"),
        ("missing-exit", "OMIT_EXIT"),
        ("missing-factory", "OMIT_FACTORY"),
    ] {
        let (bundle, ledger) = probe(name, Some(define));
        assert!(
            host.load(&bundle)
                .err()
                .unwrap()
                .to_string()
                .contains("mandatory")
        );
        assert_eq!(events(&ledger), "I");
    }
}

#[test]
fn refused_entry_and_null_factory_balance_the_attempted_sdk_ticket() {
    let host = PluginHost::windfall();
    for (name, variant, expected, error) in [
        (
            "refused-entry",
            "REFUSE_ENTRY",
            "IEX",
            "bundleEntry refused",
        ),
        ("null-factory", "NULL_FACTORY", "IEGX", "null factory"),
    ] {
        let (bundle, ledger) = probe(name, Some(variant));
        assert!(
            host.load(&bundle)
                .err()
                .unwrap()
                .to_string()
                .contains(error)
        );
        assert_eq!(events(&ledger), expected);
    }
}

#[test]
fn factory_releases_before_exit_and_refused_exit_is_persistent() {
    let (bundle, ledger) = probe("refused-exit", Some("REFUSE_EXIT"));
    let host = PluginHost::windfall();
    let module = host.load(&bundle).unwrap();
    assert!(module.descriptors().is_empty());
    drop(module);
    assert_eq!(events(&ledger), "IEGFX");
    assert!(
        host.load(&bundle)
            .err()
            .unwrap()
            .to_string()
            .contains("ExitRefused")
    );
    assert_eq!(events(&ledger), "IEGFX");
}

#[test]
fn unchanged_generation_reloads_but_retired_retarget_and_restoration_refuse() {
    let (bundle, ledger) = probe("retired-generation", None);
    let host = PluginHost::windfall();
    drop(host.load(&bundle).unwrap());
    drop(host.load(&bundle).unwrap());
    let metadata = bundle.join("Contents/Info.plist");
    let original = std::fs::read(&metadata).unwrap();
    let replacement = String::from_utf8(original.clone())
        .unwrap()
        .replace("<string>1</string>", "<string>2</string>");
    std::fs::write(&metadata, replacement).unwrap();
    let before = events(&ledger);
    assert!(
        host.load(&bundle)
            .err()
            .unwrap()
            .to_string()
            .contains("StaleSource")
    );
    std::fs::write(&metadata, original).unwrap();
    assert!(
        host.load(&bundle)
            .err()
            .unwrap()
            .to_string()
            .contains("StaleSource")
    );
    assert_eq!(events(&ledger), before);
    assert_eq!(before.matches('E').count(), 2);
    assert_eq!(before.matches('F').count(), 2);
    assert_eq!(before.matches('X').count(), 2);
}

#[test]
fn actual_fixture_bundle_path_catalog_and_instance_lifetime_are_preserved() {
    let bundle = common::plugin_file("bundle space Ω", "renamed.vst3");
    let binary = paths::vst3_binary(&bundle).unwrap();
    assert_eq!(binary.file_name().unwrap(), "windfall-fixture");
    let host = PluginHost::windfall();
    let module = host.load(&bundle).unwrap();
    let descriptors = module.descriptors();
    assert_eq!(descriptors.len(), 26);
    let scan = windfall_plugin_host::scan::scan_file(
        &common::scanner(std::time::Duration::from_secs(20)),
        &bundle,
    )
    .unwrap();
    assert!(scan.failure.is_none());
    assert_eq!(scan.plugins.len(), 26);
    assert_eq!(
        scan.plugins[1].failure.as_ref().unwrap().kind,
        windfall_plugin_host::scan::FailureKind::Rejected
    );
    for (index, descriptor) in descriptors.iter().enumerate() {
        assert_eq!(descriptor.id, format!("{:032X}", index + 1));
    }
    let mut instance = module.create(&descriptors[0].id).unwrap();
    drop(module);
    assert!(!instance.params().is_empty());
    instance.idle(&mut |_| {});
    drop(instance);
    let link = bundle.with_file_name("alias.vst3");
    std::os::unix::fs::symlink(&bundle, &link).unwrap();
    assert_eq!(
        paths::plugin_file_identity(&link).unwrap(),
        paths::plugin_file_identity(&bundle).unwrap()
    );
    let a = host.load(&bundle).unwrap();
    let b = host.load(&link).unwrap();
    drop(a);
    assert_eq!(b.descriptors().len(), 26);
    drop(b);
}

#[test]
fn foreign_live_owner_busy_and_invalid_binary_are_explicit_errors() {
    let bundle = common::plugin_file("bundle-owner-busy", "fixture.vst3");
    let module = PluginHost::windfall().load(&bundle).unwrap();
    let path = bundle.clone();
    let error = std::thread::spawn(move || {
        PluginHost::windfall()
            .load(&path)
            .err()
            .unwrap()
            .to_string()
    })
    .join()
    .unwrap();
    assert!(error.contains("Busy"));
    drop(module);
    std::thread::spawn(move || {
        drop(PluginHost::windfall().load(&bundle).unwrap());
    })
    .join()
    .unwrap();
    let folder = common::scratch().join("invalid-bundle");
    std::fs::create_dir_all(&folder).unwrap();
    let invalid = folder.join("bad-binary");
    std::fs::write(&invalid, b"not Mach-O").unwrap();
    let bundle = folder.join("invalid.vst3");
    common::macos_bundle::publish(&bundle, &invalid, "bad-binary");
    assert!(PluginHost::windfall().load(&bundle).is_err());
    assert!(PluginHost::windfall().load(&invalid).is_err());
}

struct Reentry {
    old: Option<windfall_plugin_host::PluginModule>,
    bundle: PathBuf,
    ledger: PathBuf,
    deferred: bool,
    busy: bool,
}
thread_local! {
    // Transient test callback state, explicitly removed after the second load.
    static REENTRY: std::cell::RefCell<Option<Reentry>> = const { std::cell::RefCell::new(None) };
}
extern "C" fn entry_callback() {
    // No assertion or unwrap crosses this C ABI. Report outcomes to the test
    // after native code returns, even if a candidate violates the contract.
    REENTRY.with(|slot| {
        let Ok(mut current) = slot.try_borrow_mut() else {
            return;
        };
        let Some(mut state) = current.take() else {
            return;
        };
        drop(current);
        drop(state.old.take());
        state.deferred = std::fs::read_to_string(&state.ledger)
            .is_ok_and(|events| events.replace('U', "") == "IEGE");
        state.busy = PluginHost::windfall()
            .load(&state.bundle)
            .err()
            .is_some_and(|error| error.to_string().contains("Busy"));
        if let Ok(mut current) = slot.try_borrow_mut() {
            *current = Some(state);
        }
    });
}

#[test]
fn native_entry_reentrant_drop_defers_and_reentrant_load_refuses_busy() {
    let (bundle, ledger) = probe_with_callback(
        "entry-reentry",
        Some("REENTER_ON_SECOND_ENTRY"),
        Some(entry_callback),
    );
    let host = PluginHost::windfall();
    let old = host.load(&bundle).unwrap();
    REENTRY.with(|slot| {
        *slot.borrow_mut() = Some(Reentry {
            old: Some(old),
            bundle: bundle.clone(),
            ledger: ledger.clone(),
            deferred: false,
            busy: false,
        });
    });
    let loaded = host.load(&bundle);
    // Remove the callback state even when the load returns an error.
    let state = REENTRY.with(|slot| slot.borrow_mut().take()).unwrap();
    let loaded = loaded.unwrap();
    assert!(
        state.deferred,
        "entry callback released the old factory/exit"
    );
    assert!(state.busy, "reentrant load did not explicitly refuse Busy");
    assert!(state.old.is_none());
    assert_eq!(events(&ledger), "IEGEGFX");
    drop(loaded);
    assert_eq!(events(&ledger), "IEGEGFXFX");
}
