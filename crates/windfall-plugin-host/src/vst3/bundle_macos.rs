//! CFBundle owner following Steinberg macmain.cpp/module_mac.mm (586dc5e).
//! CF declarations follow Apple CF headers (dc54c6b); native SDK probes/tests
//! remain required. CF Boolean is u8, SDK C++ bool is Rust bool, CFIndex is isize.
use super::lifecycle::{self, Cleanup, Record, Registry, Retirement, Source};
use crate::error::PluginError;
use std::cell::UnsafeCell;
use std::ffi::{CStr, c_char, c_void};
use std::io::{self, Read};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::ptr;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};
use vst3::{ComPtr, Steinberg::IPluginFactory};

type CFRef = *const c_void;
type Entry = unsafe extern "C" fn(CFRef) -> bool;
type Exit = unsafe extern "C" fn() -> bool;
type Factory = unsafe extern "system" fn() -> *mut IPluginFactory;
const UTF8: u32 = 0x0800_0100;
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: CFRef);
    fn CFGetTypeID(value: CFRef) -> usize;
    fn CFDataCreate(allocator: CFRef, bytes: *const u8, count: isize) -> CFRef;
    fn CFPropertyListCreateWithData(
        allocator: CFRef,
        data: CFRef,
        options: usize,
        format: *mut isize,
        error: *mut CFRef,
    ) -> CFRef;
    fn CFDictionaryGetTypeID() -> usize;
    fn CFDictionaryGetValue(dictionary: CFRef, key: CFRef) -> CFRef;
    fn CFStringGetTypeID() -> usize;
    fn CFStringGetLength(value: CFRef) -> isize;
    fn CFStringGetCharacterAtIndex(value: CFRef, index: isize) -> u16;
    fn CFStringCreateWithCString(allocator: CFRef, text: *const c_char, encoding: u32) -> CFRef;
    fn CFStringGetFileSystemRepresentation(value: CFRef, buffer: *mut c_char, count: isize) -> u8;
    fn CFURLCreateFromFileSystemRepresentation(
        allocator: CFRef,
        bytes: *const u8,
        count: isize,
        directory: u8,
    ) -> CFRef;
    fn CFURLGetFileSystemRepresentation(
        url: CFRef,
        resolve: u8,
        buffer: *mut u8,
        count: isize,
    ) -> u8;
    fn CFBundleCreate(allocator: CFRef, url: CFRef) -> CFRef;
    fn CFBundleCopyExecutableURL(bundle: CFRef) -> CFRef;
    fn CFBundleGetInfoDictionary(bundle: CFRef) -> CFRef;
    fn CFBundleIsExecutableLoaded(bundle: CFRef) -> u8;
    fn CFBundleLoadExecutableAndReturnError(bundle: CFRef, error: *mut CFRef) -> u8;
    fn CFBundleGetFunctionPointerForName(bundle: CFRef, name: CFRef) -> *mut c_void;
}
struct OwnedCF(CFRef);
impl OwnedCF {
    fn require(value: CFRef) -> io::Result<Self> {
        if value.is_null() {
            Err(io::Error::other("CoreFoundation returned null"))
        } else {
            Ok(Self(value))
        }
    }
}
impl Drop for OwnedCF {
    fn drop(&mut self) {
        let value = std::mem::replace(&mut self.0, ptr::null());
        if !value.is_null() {
            // SAFETY: this wrapper owns precisely one returned Create/Copy ref.
            unsafe { CFRelease(value) };
        }
    }
}
fn string(value: &CStr) -> io::Result<OwnedCF> {
    // SAFETY: nul-terminated static/owned C string, default CF allocator.
    OwnedCF::require(unsafe { CFStringCreateWithCString(ptr::null(), value.as_ptr(), UTF8) })
}
fn dictionary_string(dictionary: CFRef, key: &CStr) -> io::Result<Vec<u8>> {
    let key = string(key)?;
    // SAFETY: valid CF property list; Get returns a borrowed value.
    unsafe {
        if dictionary.is_null() || CFGetTypeID(dictionary) != CFDictionaryGetTypeID() {
            return Err(io::Error::other("Info.plist must be a dictionary"));
        }
        let value = CFDictionaryGetValue(dictionary, key.0);
        if value.is_null() || CFGetTypeID(value) != CFStringGetTypeID() {
            return Err(io::Error::other("missing/string-type bundle metadata"));
        }
        let length = CFStringGetLength(value);
        if length < 0 || length > lifecycle::MAX_PATH as isize {
            return Err(io::Error::other("macOS VST3 Capacity: metadata string"));
        }
        if (0..length).any(|index| CFStringGetCharacterAtIndex(value, index) == 0) {
            return Err(io::Error::other("NUL in bundle metadata"));
        }
        let mut buffer = [0u8; lifecycle::MAX_PATH + 1];
        if CFStringGetFileSystemRepresentation(
            value,
            buffer.as_mut_ptr().cast(),
            buffer.len() as isize,
        ) == 0
        {
            return Err(io::Error::other("bundle metadata string conversion failed"));
        }
        let end = buffer
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| io::Error::other("unterminated CF string"))?;
        Ok(buffer[..end].to_vec())
    }
}
fn plist(bytes: &[u8]) -> io::Result<OwnedCF> {
    // SAFETY: bounded supplied byte slice. The immutable plist/data and any
    // error are independent owned CF values; no bundle/native image is created.
    let data = OwnedCF::require(unsafe {
        CFDataCreate(ptr::null(), bytes.as_ptr(), bytes.len() as isize)
    })?;
    let mut error = OwnedCF(ptr::null());
    let value = unsafe {
        CFPropertyListCreateWithData(ptr::null(), data.0, 0, ptr::null_mut(), &mut error.0)
    };
    OwnedCF::require(value)
}
fn checked_path(path: &Path) -> io::Result<()> {
    let bytes = path.as_os_str().as_bytes();
    if bytes.contains(&0) || bytes.len() > lifecycle::MAX_PATH {
        return Err(io::Error::other(
            "macOS VST3 Capacity: invalid/over-limit path",
        ));
    }
    Ok(())
}
pub(crate) fn resolve_source(path: &Path) -> io::Result<Source> {
    checked_path(path)?;
    if !path.is_dir()
        || path
            .extension()
            .is_none_or(|s| !s.as_bytes().eq_ignore_ascii_case(b"vst3"))
    {
        return Err(io::Error::other(
            "macOS VST3 requires a modern .vst3 bundle directory",
        ));
    }
    let bundle = path.canonicalize()?;
    checked_path(&bundle)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve(lifecycle::MAX_PLIST + 1)
        .map_err(|_| io::Error::other("macOS VST3 Capacity: plist storage"))?;
    std::fs::File::open(bundle.join("Contents/Info.plist"))?
        .take((lifecycle::MAX_PLIST + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > lifecycle::MAX_PLIST {
        return Err(io::Error::other(
            "macOS VST3 Capacity: Info.plist too large",
        ));
    }
    let metadata = plist(&bytes)?;
    let executable = dictionary_string(metadata.0, c"CFBundleExecutable")?;
    if executable.is_empty()
        || executable.contains(&b'/')
        || executable == b"."
        || executable == b".."
    {
        return Err(io::Error::other("CFBundleExecutable must name one file"));
    }
    let binary = bundle
        .join("Contents/MacOS")
        .join(std::ffi::OsString::from_vec(executable))
        .canonicalize()?;
    checked_path(&binary)?;
    let file = std::fs::metadata(&binary)?;
    if !file.is_file() {
        return Err(io::Error::other("bundle executable is not a regular file"));
    }
    let directory = std::fs::metadata(&bundle)?;
    Ok(Source {
        bundle,
        binary,
        directory_id: (directory.dev(), directory.ino()),
        file_id: (file.dev(), file.ino()),
        size: file.len(),
        modified: (file.mtime(), file.mtime_nsec()),
        changed: (file.ctime(), file.ctime_nsec()),
        plist: bytes,
    })
}
fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(Registry::new)
}
fn refused(reason: lifecycle::Refusal) -> PluginError {
    PluginError::Load(
        windfall_core::notice!("pluginHost.macosVst3", reason = format!("{:?}", reason))
            .to_string(),
    )
}
fn load_error(error: io::Error) -> PluginError {
    PluginError::Load(error.to_string())
}
fn revalidate(source: &Source, record: &Record) -> Result<(), PluginError> {
    if resolve_source(&source.bundle).is_ok_and(|fresh| fresh == *source) {
        return Ok(());
    }
    record.stale();
    Err(refused(lifecycle::Refusal::StaleSource))
}
struct OwnedState {
    factory: Option<ComPtr<IPluginFactory>>,
    bundle: OwnedCF,
    exit: Option<Exit>,
    attempted: bool,
}
impl OwnedState {
    fn cleanup(&mut self) -> bool {
        drop(self.factory.take());
        let attempted = std::mem::replace(&mut self.attempted, false);
        let accepted = if attempted {
            // SAFETY: mandatory exit was resolved before attempt and the bundle
            // remains owned. Clearing the flag prevents a second attempted exit.
            self.exit.is_some_and(|exit| unsafe { exit() })
        } else {
            true
        };
        let bundle = std::mem::replace(&mut self.bundle, OwnedCF(ptr::null()));
        drop(bundle);
        accepted
    }
}
struct Action(Rc<UnsafeCell<OwnedState>>);
impl Cleanup for Action {
    fn cleanup(&mut self) -> bool {
        // SAFETY: lifecycle drains only after every owner's native borrow has
        // returned. This sole cleanup action owns the native retirement stages.
        unsafe { (&mut *self.0.get()).cleanup() }
    }
}
pub(super) struct MacModuleLease {
    state: Rc<UnsafeCell<OwnedState>>,
    retirement: Option<Box<Retirement>>,
}
impl MacModuleLease {
    pub fn load(path: &Path) -> Result<Self, PluginError> {
        let _preflight = registry().preflight().map_err(refused)?;
        checked_path(path).map_err(load_error)?;
        let requested = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().map_err(load_error)?.join(path)
        };
        let requested: PathBuf = requested.components().collect();
        checked_path(&requested).map_err(load_error)?;
        let known = registry().known_bundle(&requested).map_err(refused)?;
        let source = resolve_source(path).map_err(|error| {
            if let Some(known) = &known {
                known.stale();
            }
            load_error(error)
        })?;
        let ticket = registry()
            .admit_at(source.clone(), requested)
            .map_err(refused)?;
        let record = ticket.record();
        // All native cleanup storage is allocated before CFBundleCreate.
        let state = Rc::new(UnsafeCell::new(OwnedState {
            factory: None,
            bundle: OwnedCF(ptr::null()),
            exit: None,
            attempted: false,
        }));
        let lease = Self {
            state: state.clone(),
            retirement: Some(Retirement::new(ticket, Box::new(Action(state)))),
        };
        let loaded = lifecycle::scope(Some(record.clone()), || {
            if let Err(error) = lease.prepare(&source, &record) {
                record.refuse_preparation();
                return Err(error);
            }
            Ok(lease)
        })?;
        if let Some(reason) = record.refusal() {
            drop(loaded);
            return Err(refused(reason));
        }
        Ok(loaded)
    }
    fn prepare(&self, source: &Source, record: &Arc<Record>) -> Result<(), PluginError> {
        revalidate(source, record)?;
        let path = source.bundle.as_os_str().as_bytes();
        // SAFETY: bounded native filesystem bytes, default allocator, directory URL.
        let url = OwnedCF::require(unsafe {
            CFURLCreateFromFileSystemRepresentation(
                ptr::null(),
                path.as_ptr(),
                path.len() as isize,
                1,
            )
        })
        .map_err(load_error)?;
        // SAFETY: no lease is published during preparation. Its owner guard
        // already holds the reservation, including any CFPlugIn initializers.
        let state = unsafe { &mut *self.state.get() };
        state.bundle = OwnedCF(unsafe { CFBundleCreate(ptr::null(), url.0) });
        if state.bundle.0.is_null() {
            return Err(PluginError::Load("CFBundleCreate failed".into()));
        }
        let actual = OwnedCF::require(unsafe { CFBundleCopyExecutableURL(state.bundle.0) })
            .map_err(load_error)?;
        let mut path = [0u8; lifecycle::MAX_PATH + 1];
        if unsafe {
            CFURLGetFileSystemRepresentation(actual.0, 1, path.as_mut_ptr(), path.len() as isize)
        } == 0
        {
            return Err(PluginError::Load(
                "CF executable path conversion failed".into(),
            ));
        }
        let length = path
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| PluginError::Load("unterminated executable path".into()))?;
        let binary = PathBuf::from(std::ffi::OsString::from_vec(path[..length].to_vec()))
            .canonicalize()
            .map_err(load_error)?;
        let fresh = plist(&source.plist).map_err(load_error)?;
        let cached = unsafe { CFBundleGetInfoDictionary(state.bundle.0) };
        for key in [c"CFBundleExecutable", c"CFBundleIdentifier"] {
            if dictionary_string(fresh.0, key).map_err(load_error)?
                != dictionary_string(cached, key).map_err(load_error)?
            {
                record.stale();
                return Err(refused(lifecycle::Refusal::StaleSource));
            }
        }
        if binary != source.binary {
            record.stale();
            return Err(refused(lifecycle::Refusal::StaleSource));
        }
        // An externally preloaded first owner is not proof of this source.
        // Known unchanged generations can remain resident in Apple's cache.
        // The first-load check is supplied by the persistent record below.
        if unsafe { CFBundleIsExecutableLoaded(state.bundle.0) } != 0 && !record.has_loaded() {
            record.stale();
            return Err(PluginError::Load("macOS VST3 ResidentUnknown".into()));
        }
        let mut error = OwnedCF(ptr::null());
        if unsafe { CFBundleLoadExecutableAndReturnError(state.bundle.0, &mut error.0) } == 0 {
            return Err(PluginError::Load("CFBundle executable load failed".into()));
        }
        record.mark_loaded();
        let export = |name: &CStr| -> Result<*mut c_void, PluginError> {
            let name = string(name).map_err(load_error)?;
            let function = unsafe { CFBundleGetFunctionPointerForName(state.bundle.0, name.0) };
            if function.is_null() {
                Err(PluginError::Load(
                    "missing mandatory VST3 bundle export".into(),
                ))
            } else {
                Ok(function)
            }
        };
        // SAFETY: the mandatory SDK symbols have exactly these pinned signatures.
        let entry: Entry = unsafe { std::mem::transmute(export(c"bundleEntry")?) };
        let exit: Exit = unsafe { std::mem::transmute(export(c"bundleExit")?) };
        let factory: Factory = unsafe { std::mem::transmute(export(c"GetPluginFactory")?) };
        state.exit = Some(exit);
        revalidate(source, record)?;
        state.attempted = true;
        if !unsafe { entry(state.bundle.0) } {
            return Err(PluginError::Load("bundleEntry refused".into()));
        }
        state.factory = unsafe { ComPtr::from_raw(factory()) };
        if state.factory.is_none() {
            return Err(PluginError::Load("null factory".into()));
        }
        revalidate(source, record)?;
        Ok(())
    }
    pub fn with_factory<R>(&self, operation: impl FnOnce(&ComPtr<IPluginFactory>) -> R) -> R {
        let Some(node) = &self.retirement else {
            std::process::abort()
        };
        node.borrow_scope(|| {
            // SAFETY: an owner-thread scoped borrow keeps this lease alive;
            // all reentrant retirement is deferred until the scope completes.
            let state = unsafe { &*self.state.get() };
            let Some(factory) = &state.factory else {
                std::process::abort()
            };
            operation(factory)
        })
    }
}
impl Drop for MacModuleLease {
    fn drop(&mut self) {
        if let Some(node) = self.retirement.take() {
            lifecycle::retire(node);
        }
    }
}
