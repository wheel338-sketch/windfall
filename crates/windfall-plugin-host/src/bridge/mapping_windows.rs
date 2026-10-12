//! Windows pagefile-backed mappings, created and destroyed off realtime.

use super::super::{
    protocol::{REGION_BYTES, REGION_WORDS},
    slots::WordStorage,
};
use std::{
    io,
    sync::{Arc, atomic::AtomicU32},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, INVALID_HANDLE_VALUE},
    System::Memory::{
        CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
        OpenFileMappingW, PAGE_READWRITE, UnmapViewOfFile,
    },
};

pub struct Mapping {
    handle: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
}
// SAFETY: the view remains valid until the last Arc is retired; all accesses
// are aligned lock-free AtomicU32 operations. No native plugin receives it.
unsafe impl Send for Mapping {}
// SAFETY: same argument; no non-atomic access occurs while an endpoint exists.
unsafe impl Sync for Mapping {}

fn name_words(name: &str) -> io::Result<Vec<u16>> {
    super::session_suffix(name)?;
    if !name.starts_with("Local\\Windfall-Audio-")
        || name.len() > 160
        || name.contains('\0')
        || !name.is_ascii()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid bridge mapping name",
        ));
    }
    Ok(name.encode_utf16().chain(Some(0)).collect())
}
fn platform() -> io::Result<()> {
    if !cfg!(all(
        target_pointer_width = "64",
        target_endian = "little",
        target_has_atomic = "32",
        target_has_atomic = "64"
    )) || std::mem::size_of::<AtomicU32>() != 4
        || std::mem::align_of::<AtomicU32>() != 4
    {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "bridge requires verified Windows64 little-endian atomics",
        ));
    }
    Ok(())
}
impl Mapping {
    pub fn create(name: &str) -> io::Result<Arc<Self>> {
        platform()?;
        let name = name_words(name)?;
        // SAFETY: pagefile mapping, finite validated size, null default security
        // attributes and terminated name. This runs before audio installation.
        let handle = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                std::ptr::null(),
                PAGE_READWRITE,
                0,
                REGION_BYTES as u32,
                name.as_ptr(),
            )
        };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: GetLastError is consulted immediately after creation.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            // SAFETY: handle was created by this call.
            unsafe {
                CloseHandle(handle);
            }
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "bridge session mapping already exists",
            ));
        }
        Self::map(handle)
    }
    pub fn open(name: &str) -> io::Result<Arc<Self>> {
        platform()?;
        let name = name_words(name)?;
        // SAFETY: terminated checked name; handle is non-inheritable.
        let handle = unsafe { OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        Self::map(handle)
    }
    fn map(handle: HANDLE) -> io::Result<Arc<Self>> {
        // SAFETY: live mapping handle; fixed finite view size. A shorter peer
        // mapping fails instead of exposing out-of-bounds memory.
        let view = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, REGION_BYTES) };
        if view.Value.is_null() {
            let error = io::Error::last_os_error();
            // SAFETY: exclusively owned handle and failed view.
            unsafe {
                CloseHandle(handle);
            }
            return Err(error);
        }
        if !(view.Value as usize).is_multiple_of(std::mem::align_of::<AtomicU32>()) {
            // SAFETY: these resources belong to this call and were never exposed.
            unsafe {
                UnmapViewOfFile(view);
                CloseHandle(handle);
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unaligned bridge mapping",
            ));
        }
        Ok(Arc::new(Self { handle, view }))
    }
}
impl WordStorage for Mapping {
    fn words(&self) -> &[AtomicU32] {
        // SAFETY: view spans REGION_BYTES and is aligned; all bit patterns are
        // valid u32. ABI explicitly restricts both processes to the same verified
        // Windows64 architecture. Arc keeps the mapping alive through accesses.
        unsafe { std::slice::from_raw_parts(self.view.Value.cast::<AtomicU32>(), REGION_WORDS) }
    }
}
impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: final owner, no remaining endpoints. Facade retirement must
        // happen on control, never as a callback's last-Arc drop.
        unsafe {
            UnmapViewOfFile(self.view);
            CloseHandle(self.handle);
        }
    }
}
