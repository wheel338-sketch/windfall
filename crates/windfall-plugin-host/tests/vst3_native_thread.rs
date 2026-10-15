//! Native creator identity must reject another live owner and survive unload.
//!
//! The small cdylib imports the exact fixture helper. A subprocess contains
//! regressions that fault at pthread exit after its code has been unloaded.

mod common;
#[path = "../test-plugins/src/vst3/native_thread.rs"]
mod native_thread;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use native_thread::CreatorThread;

fn identity_fixture() -> &'static Path {
    static LIBRARY: OnceLock<PathBuf> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        let folder = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("native-thread-identity-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let helper = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("test-plugins/src/vst3/native_thread.rs")
            .canonicalize()
            .unwrap();
        let source = folder.join("identity_fixture.rs");
        std::fs::write(
            &source,
            format!(
                r#"#[path = {helper:?}]
mod native_thread;
use native_thread::CreatorThread;
static CREATOR: std::sync::OnceLock<CreatorThread> = std::sync::OnceLock::new();
#[unsafe(no_mangle)]
pub extern "C" fn record_creator() -> bool {{
    CREATOR.set(CreatorThread::current()).is_ok()
}}
#[unsafe(no_mangle)]
pub extern "C" fn matches_creator() -> bool {{
    CREATOR.get().is_some_and(|creator| *creator == CreatorThread::current())
}}
"#
            ),
        )
        .unwrap();
        let library = folder.join(format!(
            "{}identity_fixture{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        ));
        let compiler = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let output = Command::new(compiler)
            .args([
                "--edition=2024",
                "--crate-type=cdylib",
                "-C",
                "opt-level=1",
                "-C",
                "debuginfo=1",
            ])
            .arg(&source)
            .arg("-o")
            .arg(&library)
            .output()
            .expect("the existing Rust compiler can build the identity fixture");
        assert!(
            output.status.success(),
            "identity fixture did not compile:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        library
    })
}

#[test]
fn creator_identity_rejects_another_live_owner() {
    let creator = CreatorThread::current();
    assert_eq!(creator, CreatorThread::current());
    std::thread::spawn(move || {
        assert_ne!(
            creator,
            CreatorThread::current(),
            "a different live thread must not become the creator"
        );
    })
    .join()
    .unwrap();
    // This test's creator is still alive for every comparison above.
    assert_eq!(creator, CreatorThread::current());
}

#[test]
fn native_fixture_rejects_another_live_owner() {
    // SAFETY: the generated library exports these exact bool C signatures;
    // the library remains owned until the checking thread has joined.
    let library = unsafe { libloading::Library::new(identity_fixture()) }.unwrap();
    let record =
        unsafe { library.get::<unsafe extern "C" fn() -> bool>(b"record_creator\0") }.unwrap();
    let matches =
        unsafe { library.get::<unsafe extern "C" fn() -> bool>(b"matches_creator\0") }.unwrap();
    assert!(unsafe { record() });
    assert!(unsafe { matches() });
    let matches = *matches;
    assert!(
        std::thread::spawn(move || !unsafe { matches() })
            .join()
            .unwrap(),
        "the native fixture must reject a different live owner"
    );
    assert!(unsafe { matches() });
    drop(library);
}

#[test]
fn native_component_owner_checks_reject_another_live_owner() {
    const CHILD_LIBRARY: &str = "WINDFALL_NATIVE_COMPONENT_OWNER_CHILD";
    const CHILD_METHOD: &str = "WINDFALL_NATIVE_COMPONENT_OWNER_METHOD";
    if let Some(library_path) = std::env::var_os(CHILD_LIBRARY) {
        use vst3::Steinberg::Vst::{IComponent, IComponentTrait};
        use vst3::Steinberg::{
            IPluginBaseTrait, IPluginFactory, IPluginFactoryTrait, PClassInfo, kResultOk,
        };
        use vst3::{ComPtr, Interface};

        // This known fixture's entry exports are no-ops. Call its real factory
        // directly to reach the native owner guards without a host-side owner
        // check rejecting the intentional misuse first. No other lifecycle or
        // audio operation runs concurrently with the foreign-thread call.
        let library = unsafe { libloading::Library::new(library_path) }.unwrap();
        let get = unsafe {
            library.get::<unsafe extern "system" fn() -> *mut IPluginFactory>(b"GetPluginFactory\0")
        }
        .unwrap();
        // SAFETY: the exact SDK export returns an owned factory reference;
        // its executable remains loaded through both component references.
        let factory = unsafe { ComPtr::from_raw(get()) }.unwrap();
        let mut class: PClassInfo = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { factory.getClassInfo(0, &mut class) }, kResultOk);
        let mut raw = std::ptr::null_mut();
        assert_eq!(
            unsafe {
                factory.createInstance(
                    class.cid.as_ptr(),
                    IComponent::IID.as_ptr().cast(),
                    &mut raw,
                )
            },
            kResultOk
        );
        // SAFETY: createInstance provided this owned IComponent reference.
        let component = unsafe { ComPtr::<IComponent>::from_raw(raw.cast()) }.unwrap();
        let address = component.as_ptr() as usize;
        let method = std::env::var(CHILD_METHOD).unwrap();
        std::thread::spawn(move || {
            // SAFETY: the creator retains the component and library until
            // this thread joins. These known fixture methods first read the
            // immutable creator guard and must abort on this wrong live owner.
            let borrowed =
                unsafe { vst3::ComRef::<IComponent>::from_raw(address as *mut _) }.unwrap();
            match method.as_str() {
                "setActive" => assert_eq!(unsafe { borrowed.setActive(0) }, kResultOk),
                "terminate" => assert_eq!(unsafe { borrowed.terminate() }, kResultOk),
                _ => unreachable!(),
            }
        })
        .join()
        .unwrap();
        // If a guard was removed or identity became constant, the child would
        // complete successfully and the parent regression below would fail.
        drop(component);
        drop(factory);
        drop(library);
        return;
    }

    let bundle = common::plugin_file("vst3-native-owner", "fixture.vst3");
    // On macOS the fixture is published as a bundle; the child loads its
    // executable directly.
    let library = if cfg!(target_os = "macos") {
        bundle.join("Contents/MacOS/windfall-fixture")
    } else {
        bundle
    };
    for (method, original_message) in [
        ("setActive", "native lifecycle left the creating owner"),
        ("terminate", "native destruction left the creating owner"),
    ] {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "native_component_owner_checks_reject_another_live_owner",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_LIBRARY, &library)
            .env(CHILD_METHOD, method)
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "native {method} accepted a different live owner"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(original_message),
            "native {method} did not reach its original owner assertion: {}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn native_identity_survives_unload_before_worker_exit() {
    const CHILD_LIBRARY: &str = "WINDFALL_NATIVE_IDENTITY_UNLOAD_CHILD";
    if let Some(library_path) = std::env::var_os(CHILD_LIBRARY) {
        std::thread::spawn(move || {
            // SAFETY: the parent builds the exact C exports loaded here.
            let library = unsafe { libloading::Library::new(library_path) }.unwrap();
            let record =
                unsafe { library.get::<unsafe extern "C" fn() -> bool>(b"record_creator\0") }
                    .unwrap();
            let matches =
                unsafe { library.get::<unsafe extern "C" fn() -> bool>(b"matches_creator\0") }
                    .unwrap();
            assert!(unsafe { record() });
            assert!(unsafe { matches() });
            // End every borrowed symbol before releasing the only library
            // owner. The worker then exits and the parent must join it.
            drop(library);
        })
        .join()
        .unwrap();
        return;
    }
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "native_identity_survives_unload_before_worker_exit",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_LIBRARY, identity_fixture())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "native identity cleanup failed after dlclose and worker exit: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
