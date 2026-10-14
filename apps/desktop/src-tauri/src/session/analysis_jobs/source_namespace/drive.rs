//! Retain the effective drive object before resolving any file components.
//! Chained/substituted DOS mappings are deliberately unsupported: one retained
//! object cannot establish authority over every mapping in such a chain.
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Component, Path, Prefix};

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}
#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root: *mut std::ffi::c_void,
    name: *mut UnicodeString,
    attributes: u32,
    security_descriptor: *mut std::ffi::c_void,
    security_qos: *mut std::ffi::c_void,
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtOpenSymbolicLinkObject(
        handle: *mut *mut std::ffi::c_void,
        access: u32,
        attributes: *mut ObjectAttributes,
    ) -> i32;
    fn NtQuerySymbolicLinkObject(
        handle: *mut std::ffi::c_void,
        target: *mut UnicodeString,
        returned: *mut u32,
    ) -> i32;
}

fn error(message: impl std::fmt::Display) -> String {
    super::super::error("sourceNamespace", message)
}

pub(super) fn pin(path: &Path) -> Result<OwnedHandle, String> {
    if !path.is_absolute() || path.as_os_str().len() > 1024 {
        return Err(error("Expected a bounded absolute local path."));
    }
    if path
        .parent()
        .is_none_or(|parent| parent.components().count() > 65)
    {
        return Err(error(
            "Analysis namespace limit is 64 parent directory handles.",
        ));
    }
    let letter = match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter,
            _ => return Err(error("Analysis requires a local drive namespace.")),
        },
        _ => return Err(error("Analysis requires a local drive namespace.")),
    };
    let mut name: Vec<u16> = format!(r"\??\{}:", char::from(letter))
        .encode_utf16()
        .collect();
    let bytes = (name.len() * 2) as u16;
    let mut name = UnicodeString {
        length: bytes,
        maximum_length: bytes,
        buffer: name.as_mut_ptr(),
    };
    let mut attributes = ObjectAttributes {
        length: size_of::<ObjectAttributes>() as u32,
        root: std::ptr::null_mut(),
        name: &mut name,
        attributes: 0x40, // OBJ_CASE_INSENSITIVE; open the effective DOS object.
        security_descriptor: std::ptr::null_mut(),
        security_qos: std::ptr::null_mut(),
    };
    let mut raw = std::ptr::null_mut();
    // SAFETY: bounded UTF-16 name, nested inputs and output pointer remain live.
    // Only a successful, non-null handle becomes an exactly-once owned handle.
    let status = unsafe { NtOpenSymbolicLinkObject(&mut raw, 1, &mut attributes) };
    if status < 0 || raw.is_null() {
        return Err(error(windfall_ipc::ui_message(
            "analysis.driveObject",
            serde_json::json!({"status": format!("{:#x}", status)}),
        )));
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut buffer = [0u16; 1024];
    let mut target = UnicodeString {
        length: 0,
        maximum_length: (buffer.len() * 2) as u16,
        buffer: buffer.as_mut_ptr(),
    };
    let mut returned = 0;
    // SAFETY: the retained object and bounded writable UTF-16 output remain live.
    let status =
        unsafe { NtQuerySymbolicLinkObject(handle.as_raw_handle(), &mut target, &mut returned) };
    if status < 0
        || usize::from(target.length) > buffer.len() * 2
        || !target.length.is_multiple_of(2)
    {
        return Err(error("Cannot query the retained drive namespace."));
    }
    let target = String::from_utf16(&buffer[..usize::from(target.length) / 2])
        .map_err(|_| error("Invalid drive namespace target."))?;
    let physical = target
        .strip_prefix(r"\Device\HarddiskVolume")
        .is_some_and(|volume| !volume.is_empty() && volume.bytes().all(|b| b.is_ascii_digit()));
    if !physical {
        return Err(error(
            "Analysis refuses substituted or chained DOS drive mappings. Use an ordinary local physical-volume source path.",
        ));
    }
    Ok(handle)
}
