fn main() {
    // Let the linker own the single executable manifest for both the app and
    // library-test executables. Tauri still emits its icon/version resources.
    tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(
        tauri_build::WindowsAttributes::new_without_app_manifest(),
    ))
    .expect("Tauri build configuration");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // Tauri's executable manifest enables Common Controls v6. Library
        // unit-test executables also link the real dialog APIs (TaskDialogIndirect)
        // through the mock runtime and need the same loader dependency before
        // main runs. Emit it for all linked targets, including library tests.
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
}
