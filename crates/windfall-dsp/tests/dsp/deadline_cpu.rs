//! Test-only CPU admission: a budget is a share of one performance core.
//! No process affinity, priority, workload or budget changes. CPU-set selection
//! is restored on the measuring thread, including unwinding. API documentation:
//! https://learn.microsoft.com/windows/win32/api/winnt/ns-winnt-system_cpu_set_information

use std::time::Duration;

pub fn report(label: &str, times: &mut [Duration], unrestricted: Duration) -> Duration {
    assert_eq!(times.len(), 3);
    times.sort_unstable();
    eprintln!(
        "{label}: minimum={:?} median={:?} worst={:?}; unrestricted={unrestricted:?} (information only)",
        times[0], times[1], times[2]
    );
    times[0]
}

#[cfg(not(windows))]
pub struct PerformanceCore;

#[cfg(not(windows))]
impl PerformanceCore {
    pub fn enter() -> Self {
        eprintln!("deadline CPU: unrestricted (non-Windows)");
        Self
    }
}

#[cfg(windows)]
pub use windows::PerformanceCore;

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;
    use std::marker::PhantomData;
    use std::rc::Rc;

    type Query = unsafe extern "system" fn(*mut c_void, u32, *mut u32, *mut c_void, u32) -> i32;
    type Get = unsafe extern "system" fn(*mut c_void, *mut u32, u32, *mut u32) -> i32;
    type Set = unsafe extern "system" fn(*mut c_void, *const u32, u32) -> i32;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
        fn GetCurrentThread() -> *mut c_void;
        fn GetCurrentProcess() -> *mut c_void;
    }

    #[derive(Debug, PartialEq, Eq)]
    struct Core { id: u32, group: u16, logical: u8, class: u8 }

    fn fastest(mut bytes: &[u8]) -> Option<Vec<Core>> {
        let mut cores = Vec::new();
        while !bytes.is_empty() {
            let size = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
            if size < 8 || size > bytes.len() { return None; }
            let kind = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
            if kind == 0 {
                if size < 32 { return None; }
                // Ignore sets allocated exclusively to another process.
                if bytes[19] & 2 == 0 || bytes[19] & 4 != 0 {
                    cores.push(Core {
                        id: u32::from_le_bytes(bytes[8..12].try_into().ok()?),
                        group: u16::from_le_bytes(bytes[12..14].try_into().ok()?),
                        logical: bytes[14], class: bytes[18],
                    });
                }
            }
            bytes = &bytes[size..];
        }
        let class = cores.iter().map(|c| c.class).max()?;
        cores.retain(|c| c.class == class);
        Some(cores)
    }

    pub struct PerformanceCore {
        restore: Option<(Set, Vec<u32>)>,
        // Pseudo thread handles and restoration are valid on this thread only.
        _thread: PhantomData<Rc<()>>,
    }

    impl PerformanceCore {
        pub fn enter() -> Self {
            let restore = Self::select();
            if restore.is_none() { eprintln!("deadline CPU: unrestricted (CPU-set query/selection unavailable)"); }
            Self { restore, _thread: PhantomData }
        }

        fn select() -> Option<(Set, Vec<u32>)> {
            let name: Vec<u16> = "kernel32.dll\0".encode_utf16().collect();
            // SAFETY: NUL-terminated strings; kernel32 stays loaded for the process.
            let module = unsafe { GetModuleHandleW(name.as_ptr()) };
            if module.is_null() { return None; }
            // SAFETY: exported symbol lookup; checked for absence before conversion.
            let query = unsafe { GetProcAddress(module, c"GetSystemCpuSetInformation".as_ptr().cast()) };
            // SAFETY: same module and NUL-terminated symbol.
            let get = unsafe { GetProcAddress(module, c"GetThreadSelectedCpuSets".as_ptr().cast()) };
            // SAFETY: same module and NUL-terminated symbol.
            let set = unsafe { GetProcAddress(module, c"SetThreadSelectedCpuSets".as_ptr().cast()) };
            if query.is_null() || get.is_null() || set.is_null() { return None; }
            // SAFETY: signatures exactly match the named Windows APIs.
            let (query, get, set): (Query, Get, Set) = unsafe {
                (std::mem::transmute(query), std::mem::transmute(get), std::mem::transmute(set))
            };
            // SAFETY: Windows pseudo handles refer to this process and thread.
            let process = unsafe { GetCurrentProcess() };
            // SAFETY: same-thread pseudo handle, never transferred.
            let thread = unsafe { GetCurrentThread() };
            let mut length = 0;
            // SAFETY: documented size query with a valid output pointer.
            unsafe { query(std::ptr::null_mut(), 0, &mut length, process, 0); }
            if length == 0 || length > 1024 * 1024 { return None; }
            // u64 backing provides the native structure's alignment.
            let mut buffer = vec![0_u64; (length as usize).div_ceil(8)];
            let capacity = (buffer.len() * 8) as u32;
            // SAFETY: aligned writable buffer of the declared capacity.
            if unsafe { query(buffer.as_mut_ptr().cast(), capacity, &mut length, process, 0) } == 0 || length > capacity { return None; }
            // SAFETY: bytes initialized by the successful query, bounded above.
            let bytes = unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast(), length as usize) };
            let cores = fastest(bytes)?;
            let selected: Vec<u32> = cores.iter().map(|c| c.id).collect();
            let mut count = 0;
            // SAFETY: documented size query; count is writable.
            unsafe { get(thread, std::ptr::null_mut(), 0, &mut count); }
            if count > 32768 { return None; }
            let mut prior = vec![0_u32; count as usize];
            // SAFETY: writable buffer with requested count, or null for zero.
            let prior_ptr = if prior.is_empty() { std::ptr::null_mut() } else { prior.as_mut_ptr() };
            if unsafe { get(thread, prior_ptr, prior.len() as u32, &mut count) } == 0 || count as usize > prior.len() { return None; }
            prior.truncate(count as usize);
            // SAFETY: current thread and valid CPU-set IDs from the system query.
            if unsafe { set(thread, selected.as_ptr(), selected.len() as u32) } == 0 { return None; }
            eprintln!("deadline CPU: highest efficiency class; selected {cores:?}");
            Some((set, prior))
        }
    }

    impl Drop for PerformanceCore {
        fn drop(&mut self) {
            if let Some((set, prior)) = &self.restore {
                let ids = if prior.is_empty() { std::ptr::null() } else { prior.as_ptr() };
                // SAFETY: restored on the owning thread; null/zero clears selection.
                let restored = unsafe { set(GetCurrentThread(), ids, prior.len() as u32) };
                assert_ne!(restored, 0, "failed to restore deadline thread CPU sets");
            }
        }
    }

    #[test]
    fn variable_cpu_records_select_highest_available_class_and_reject_malformed_data() {
        fn record(id: u32, class: u8, flags: u8) -> [u8; 40] {
            let mut b = [0; 40]; b[..4].copy_from_slice(&40_u32.to_le_bytes());
            b[8..12].copy_from_slice(&id.to_le_bytes()); b[18] = class; b[19] = flags; b
        }
        let bytes: Vec<_> = [record(1, 0, 0), record(2, 1, 0), record(3, 2, 2), record(4, 1, 6)].into_iter().flatten().collect();
        assert_eq!(fastest(&bytes).unwrap().iter().map(|c| c.id).collect::<Vec<_>>(), [2, 4]);
        assert!(fastest(&bytes[..bytes.len()-1]).is_none());
        assert!(fastest(&[0; 32]).is_none());
    }
}
