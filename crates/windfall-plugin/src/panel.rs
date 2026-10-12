//! Main-thread control panel. Hosts can load an exported program without a
//! second audio device or desktop process. Native parent owns the outer window.
use crate::publication::Publication;
#[cfg(windows)]
use std::ffi::CStr;
use std::{
    ffi::{c_char, c_void},
    rc::Rc,
};
pub fn supports(platform: *const c_char) -> bool {
    if platform.is_null() {
        return false;
    }
    #[cfg(windows)]
    {
        unsafe { CStr::from_ptr(platform) == c"HWND" }
    }
    #[cfg(not(windows))]
    {
        let _ = platform;
        false
    }
}
#[cfg(windows)]
pub use windows::Panel;
#[cfg(not(windows))]
pub struct Panel;
#[cfg(not(windows))]
impl Panel {
    pub fn open(_: *mut c_void, _: Rc<Publication>) -> Result<Self, String> {
        Err("Native export panel requires Windows".into())
    }
    pub fn visible(&self, _: bool) {}
}
#[cfg(windows)]
mod windows {
    use super::*;
    use std::{io::Read, ptr};
    use windows_sys::Win32::{
        Foundation::*, Graphics::Gdi::COLOR_WINDOW, System::LibraryLoader::*,
        UI::WindowsAndMessaging::*,
    };
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }
    struct Context {
        state: Rc<Publication>,
        path: HWND,
        status: HWND,
    }
    pub struct Panel {
        window: HWND,
        context: Box<Context>,
        class: Vec<u16>,
        module: HINSTANCE,
    }
    unsafe extern "system" fn window_proc(
        window: HWND,
        message: u32,
        w: WPARAM,
        l: LPARAM,
    ) -> LRESULT {
        if message == WM_NCCREATE {
            let create = unsafe { &*(l as *const CREATESTRUCTW) };
            unsafe {
                SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
            }
        }
        if message == WM_COMMAND && w & 0xffff == 1 && w >> 16 == 0 {
            let context =
                unsafe { (GetWindowLongPtrW(window, GWLP_USERDATA) as *mut Context).as_mut() };
            if let Some(context) = context {
                let mut text = [0u16; 4096];
                let size =
                    unsafe { GetWindowTextW(context.path, text.as_mut_ptr(), text.len() as i32) };
                let path = String::from_utf16_lossy(&text[..size.max(0) as usize]);
                let result = (|| {
                    let file = std::fs::File::open(path.trim()).map_err(|e| e.to_string())?;
                    let mut bytes = Vec::new();
                    file.take(crate::MAX_STATE_BYTES as u64 + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|e| e.to_string())?;
                    context.state.load(crate::Program::decode(&bytes)?)
                })();
                let label = result.err().unwrap_or_else(|| {
                    "Program queued. Host transport and tempo drive playback.".into()
                });
                unsafe {
                    SetWindowTextW(context.status, wide(&label).as_ptr());
                }
            }
            return 0;
        }
        unsafe { DefWindowProcW(window, message, w, l) }
    }
    impl Panel {
        pub fn visible(&self, visible: bool) {
            unsafe {
                ShowWindow(self.window, if visible { SW_SHOW } else { SW_HIDE });
            }
        }
        pub fn open(parent: *mut c_void, state: Rc<Publication>) -> Result<Self, String> {
            if parent.is_null() {
                return Err("Missing host editor parent".into());
            }
            let mut context = Box::new(Context {
                state,
                path: ptr::null_mut(),
                status: ptr::null_mut(),
            });
            let mut module = ptr::null_mut();
            if unsafe {
                GetModuleHandleExW(
                    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS
                        | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                    window_proc as *const u16,
                    &mut module,
                )
            } == 0
            {
                return Err("Panel module lookup failed".into());
            }
            // Every view owns its class, so no registered callback points into
            // an unloaded plugin DLL, even when both formats are loaded at once.
            let class = wide(&format!("WindfallExportControlPanel-{:p}", &*context));
            let native = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(window_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: module,
                hIcon: ptr::null_mut(),
                hCursor: unsafe { LoadCursorW(ptr::null_mut(), IDC_ARROW) },
                hbrBackground: (COLOR_WINDOW + 1) as _,
                lpszMenuName: ptr::null(),
                lpszClassName: class.as_ptr(),
            };
            if unsafe { RegisterClassW(&native) } == 0 {
                return Err("Panel window registration failed".into());
            }
            let window = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    wide("Windfall").as_ptr(),
                    WS_CHILD | WS_VISIBLE,
                    0,
                    0,
                    560,
                    180,
                    parent,
                    ptr::null_mut(),
                    module,
                    (&mut *context as *mut Context).cast(),
                )
            };
            if window.is_null() {
                unsafe {
                    UnregisterClassW(class.as_ptr(), module);
                }
                return Err("Host panel creation failed".into());
            }
            let child = |class: &str, label: &str, style: u32, x, y, width, height, id: usize| unsafe {
                CreateWindowExW(
                    0,
                    wide(class).as_ptr(),
                    wide(label).as_ptr(),
                    WS_CHILD | WS_VISIBLE | style,
                    x,
                    y,
                    width,
                    height,
                    window,
                    id as _,
                    module,
                    ptr::null(),
                )
            };
            child(
                "STATIC",
                "Windfall — 16 stereo outputs",
                0,
                12,
                12,
                530,
                24,
                0,
            );
            child(
                "STATIC",
                "Exported program state file (windfall-state.json):",
                0,
                12,
                44,
                530,
                20,
                0,
            );
            context.path = child(
                "EDIT",
                "",
                WS_BORDER | WS_TABSTOP | ES_AUTOHSCROLL as u32,
                12,
                68,
                420,
                26,
                2,
            );
            child("BUTTON", "Load program", WS_TABSTOP, 436, 68, 112, 26, 1);
            context.status = child(
                "STATIC",
                "Host transport, tempo and meter take precedence over the document.",
                0,
                12,
                108,
                530,
                58,
                0,
            );
            if context.path.is_null() || context.status.is_null() {
                unsafe {
                    DestroyWindow(window);
                    UnregisterClassW(class.as_ptr(), module);
                }
                return Err("Panel controls failed".into());
            }
            Ok(Self {
                window,
                context,
                class,
                module,
            })
        }
    }
    impl Drop for Panel {
        fn drop(&mut self) {
            unsafe {
                DestroyWindow(self.window);
                UnregisterClassW(self.class.as_ptr(), self.module);
            }
            let _ = &self.context;
        }
    }
}
