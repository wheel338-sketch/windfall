//! macOS/Linux shared, file-backed mmap. Private 0700 session directory and
//! exclusive 0600 backing file; only its creating supervisor removes the name.
use super::super::{protocol::{REGION_BYTES, REGION_WORDS}, slots::WordStorage};
use std::{ffi::{c_void, c_int}, fs::{File, OpenOptions, DirBuilder}, io, os::unix::{fs::{DirBuilderExt, OpenOptionsExt}, io::AsRawFd}, path::PathBuf, sync::{Arc, atomic::AtomicU32}};
unsafe extern "C" {
    fn mmap(address: *mut c_void, length: usize, protection: c_int, flags: c_int, fd: c_int, offset: i64) -> *mut c_void;
    fn munmap(address: *mut c_void, length: usize) -> c_int;
}
pub struct Mapping { view: *mut c_void, _file: File, directory: PathBuf, owner: bool }
// SAFETY: aligned AtomicU32-only shared access; Arc owns the live mapping.
unsafe impl Send for Mapping {}
// SAFETY: every published ABI word is accessed atomically by both processes.
unsafe impl Sync for Mapping {}
fn directory(name: &str) -> io::Result<PathBuf> {
    Ok(std::env::temp_dir().join(format!("windfall-audio-{}", super::session_suffix(name)?)))
}
fn options() -> OpenOptions {
    let mut options = OpenOptions::new(); options.read(true).write(true).mode(0o600);
    // O_NOFOLLOW is platform-specific; refusal avoids opening a substituted link.
    #[cfg(target_os = "linux")] options.custom_flags(0x20000);
    #[cfg(target_os = "macos")] options.custom_flags(0x100);
    options
}
impl Mapping {
    pub fn create(name: &str) -> io::Result<Arc<Self>> {
        let directory = directory(name)?;
        DirBuilder::new().mode(0o700).create(&directory)?;
        let result = (|| {
            let file = options().create_new(true).open(directory.join("words"))?;
            file.set_len(REGION_BYTES as u64)?;
            Self::map(file, directory.clone(), true)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(directory.join("words"));
            let _ = std::fs::remove_dir(directory);
        }
        result
    }
    pub fn open(name: &str) -> io::Result<Arc<Self>> {
        let directory = directory(name)?;
        let file = options().open(directory.join("words"))?;
        Self::map(file, directory, false)
    }
    fn map(file: File, directory: PathBuf, owner: bool) -> io::Result<Arc<Self>> {
        if !cfg!(all(target_pointer_width = "64", target_endian = "little", target_has_atomic = "32", target_has_atomic = "64"))
            || size_of::<AtomicU32>() != 4 || align_of::<AtomicU32>() != 4
            || !file.metadata()?.is_file() || file.metadata()?.len() != REGION_BYTES as u64 {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "bridge requires exact 64-bit little-endian shared-word mapping"));
        }
        // SAFETY: read/write file spans the complete region, offset zero. Unix
        // PROT_READ|PROT_WRITE=3 and MAP_SHARED=1 on both supported targets.
        let view = unsafe { mmap(std::ptr::null_mut(), REGION_BYTES, 3, 1, file.as_raw_fd(), 0) };
        if view as isize == -1 { return Err(io::Error::last_os_error()); }
        if !(view as usize).is_multiple_of(align_of::<AtomicU32>()) {
            unsafe { munmap(view, REGION_BYTES); }
            return Err(io::Error::new(io::ErrorKind::InvalidData, "unaligned bridge mapping"));
        }
        Ok(Arc::new(Self { view, _file: file, directory, owner }))
    }
}
impl WordStorage for Mapping {
    fn words(&self) -> &[AtomicU32] {
        // SAFETY: mapped exact length, aligned atomics; Arc retains its lifetime.
        unsafe { std::slice::from_raw_parts(self.view.cast(), REGION_WORDS) }
    }
}
impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: last endpoint is retired by the control owner.
        unsafe { munmap(self.view, REGION_BYTES); }
        if self.owner {
            let _ = std::fs::remove_file(self.directory.join("words"));
            let _ = std::fs::remove_dir(&self.directory);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    #[test]
    fn separate_unix_mapping_views_share_words_and_owner_alone_removes_the_name() {
        let name = format!("Local\\Windfall-Audio-{:016x}-{:016x}", std::process::id(), 0x726f756e6434_u64);
        let parent = Mapping::create(&name).unwrap();
        let child = Mapping::open(&name).unwrap();
        parent.words()[123].store(0x12345678, Ordering::Release);
        assert_eq!(child.words()[123].load(Ordering::Acquire), 0x12345678);
        child.words()[124].store(0x87654321, Ordering::Release);
        assert_eq!(parent.words()[124].load(Ordering::Acquire), 0x87654321);
        assert!(Mapping::create(&name).is_err());
        drop(child);
        assert!(directory(&name).unwrap().join("words").exists());
        drop(parent);
        assert!(!directory(&name).unwrap().exists());
    }
}
