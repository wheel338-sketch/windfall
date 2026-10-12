//! VST3 scanning exercises an actual independent SDK factory fixture.
#![cfg(any(windows, target_os = "linux", target_os = "macos"))]
mod common;
use std::time::Duration;
use windfall_plugin_host::scan::FailureKind;
use windfall_plugin_host::{PluginFormat, PluginHost, PluginKind, scan_file};

#[test]
fn runtime_repair_bundle_catalog_keeps_scanned_binary_identity_until_refresh() {
    use windfall_plugin_host::{paths, scan::PluginCatalog};
    let folder = common::scratch().join("bundle-cache");
    let bundle = folder.join("Bundle.vst3");
    #[cfg(not(target_os = "macos"))]
    let (architecture, binary_name) = if cfg!(all(windows, target_arch = "aarch64")) {
        ("arm64-win", "Bundle.vst3")
    } else if cfg!(all(windows, target_arch = "x86")) {
        ("x86-win", "Bundle.vst3")
    } else if cfg!(windows) {
        ("x86_64-win", "Bundle.vst3")
    } else if cfg!(target_arch = "aarch64") {
        ("aarch64-linux", "Bundle.so")
    } else {
        ("x86_64-linux", "Bundle.so")
    };
    #[cfg(not(target_os = "macos"))]
    let binary = bundle.join("Contents").join(architecture).join(binary_name);
    #[cfg(not(target_os = "macos"))]
    std::fs::create_dir_all(binary.parent().unwrap()).unwrap();
    #[cfg(not(target_os = "macos"))]
    std::fs::copy(common::test_plugins(), &binary).unwrap();
    #[cfg(target_os = "macos")]
    let binary = {
        std::fs::create_dir_all(&folder).unwrap();
        let source = common::plugin_file("vst3-catalog-source", "fixture.vst3");
        common::macos_bundle::publish(
            &bundle,
            &paths::vst3_binary(&source).unwrap(),
            "catalog-executable",
        );
        bundle.join("Contents/MacOS/catalog-executable")
    };
    let files = paths::find_plugins(std::slice::from_ref(&folder));
    let runner = common::scanner(Duration::from_secs(20));
    let mut catalog = PluginCatalog::new();
    assert_eq!(
        catalog
            .refresh(&files, &runner, &mut |_| {})
            .unwrap()
            .scanned,
        1
    );
    let approved = catalog.identity(&bundle).unwrap().clone();
    assert_eq!(approved.binary, std::fs::canonicalize(&binary).unwrap());
    let cache = folder.join("catalog.json");
    catalog.save(&cache).unwrap();
    let mut catalog = PluginCatalog::load(&cache);
    assert_eq!(catalog.identity(&bundle), Some(&approved));
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&binary)
        .unwrap()
        .write_all(b"changed binary overlay")
        .unwrap();
    assert_ne!(paths::plugin_file_identity(&bundle).unwrap(), approved);
    assert_eq!(
        catalog.identity(&bundle),
        Some(&approved),
        "approval must retain the scanned stamp"
    );
    assert_eq!(
        catalog
            .refresh(&files, &runner, &mut |_| {})
            .unwrap()
            .scanned,
        1
    );
    assert_eq!(
        catalog.identity(&bundle),
        Some(&paths::plugin_file_identity(&bundle).unwrap())
    );
    std::fs::remove_file(binary).unwrap();
    assert!(paths::plugin_file_identity(&bundle).is_err());
    assert_eq!(
        catalog
            .refresh(&files, &runner, &mut |_| {})
            .unwrap()
            .removed,
        1
    );
    assert!(catalog.identity(&bundle).is_none());
}

#[test]
fn vst3_factory_metadata_and_initialized_buses_are_scanned_out_of_process() {
    let path = common::plugin_file("vst3", "fixture.vst3");
    let scan = scan_file(&common::scanner(Duration::from_secs(20)), &path).unwrap();
    assert!(scan.failure.is_none());
    assert_eq!(scan.plugins.len(), 24);
    let good = &scan.plugins[0];
    assert_eq!(good.descriptor.format, PluginFormat::Vst3);
    assert_eq!(good.descriptor.kind, PluginKind::Effect);
    assert_eq!(good.descriptor.vendor, "Windfall Tests");
    assert_eq!(good.descriptor.version, "1.0");
    assert_eq!(good.descriptor.features, ["Fx", "Tools"]);
    let layout = good.layout.as_ref().unwrap();
    assert_eq!(layout.audio_inputs[0].channels, 2);
    assert_eq!(layout.audio_outputs[0].name, "Stereo");
    assert!(layout.audio_outputs[0].main);
    assert_eq!(
        scan.plugins[1].failure.as_ref().unwrap().kind,
        FailureKind::Rejected
    );
    let host = PluginHost::windfall();
    let module = host.load(&path).unwrap();
    let instance = module.create(&good.descriptor.id).unwrap();
    assert_eq!(instance.params()[0].id, 7);
    assert_eq!(scan.plugins[2].descriptor.kind, PluginKind::Instrument);
}
