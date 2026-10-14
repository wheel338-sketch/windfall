//! Main-thread control panel. Hosts can load an exported program without a
//! second audio device or desktop process. Native parent owns the outer window.
use crate::publication::Publication;
use std::ffi::CStr;
use std::{
    ffi::{c_char, c_void},
    rc::Rc,
};
use windfall_core::english::{Language, in_language};
pub fn supports(platform: *const c_char) -> bool {
    if platform.is_null() {
        return false;
    }
    #[cfg(windows)]
    {
        unsafe {
            matches!(CStr::from_ptr(platform), value if value == c"HWND" || value == c"win32")
        }
    }
    #[cfg(target_os = "macos")]
    {
        unsafe {
            matches!(CStr::from_ptr(platform), value if value == c"NSView" || value == c"cocoa")
        }
    }
    #[cfg(target_os = "linux")]
    {
        unsafe {
            matches!(CStr::from_ptr(platform), value if value == c"X11EmbedWindowID" || value == c"x11")
        }
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = platform;
        false
    }
}
#[cfg(windows)]
pub use windows::Panel;
#[cfg(target_os = "macos")]
mod cocoa;
#[cfg(target_os = "macos")]
pub use cocoa::Panel;
#[cfg(target_os = "macos")]
pub(crate) use cocoa::preferred_language;
#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
pub use x11::Panel;
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub struct Panel;
#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
impl Panel {
    pub fn open(_: *mut c_void, _: Rc<Publication>) -> Result<Self, String> {
        Err("Native export panel requires Windows, macOS or X11".into())
    }
    pub fn visible(&self, _: bool) {}
}
/// What the panels say, in the language the user chose in Windfall
/// (`crate::language`), from the interface's own catalogs: the panel's own
/// words and every coded message it shows. A name or path somebody typed
/// and what the operating system says stay as they are.
#[derive(Debug, Clone, Copy)]
struct Words(Language);
impl Words {
    fn chosen() -> Self {
        Self(crate::language::chosen())
    }
    fn say(self, text: &str) -> String {
        in_language(text, self.0)
    }
    fn title(self) -> String {
        self.say(&windfall_core::notice!("plugin.panel.title").to_string())
    }
    fn state_file(self) -> String {
        self.say(&windfall_core::notice!("plugin.panel.stateFile").to_string())
    }
    fn load(self) -> String {
        self.say(&windfall_core::notice!("plugin.panel.load").to_string())
    }
    fn queued(self) -> String {
        self.say(&windfall_core::notice!("plugin.panel.queued").to_string())
    }
    fn host_first(self) -> String {
        self.say(&windfall_core::notice!("plugin.panel.hostFirst").to_string())
    }
    /// What the status line says after loading the program at `path`.
    fn loaded(self, state: &Publication, path: &str) -> String {
        load_program(state, path, self)
            .err()
            .unwrap_or_else(|| status_text(state, self.queued(), self))
    }
}
/// The same bounded load transaction is used by all three native panels.
fn load_program(state: &Publication, path: &str, words: Words) -> Result<(), String> {
    let file = std::fs::File::open(path.trim()).map_err(|e| e.to_string())?;
    load_reader(state, file).map_err(|error| words.say(&error))
}
fn load_reader(state: &Publication, reader: impl std::io::Read) -> Result<(), String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    reader
        .take(crate::MAX_STATE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    state.load_editor(crate::Program::decode(&bytes)?)
}
fn status_text(state: &Publication, ready: String, words: Words) -> String {
    state
        .preparation_error()
        .map(|error| words.say(error))
        .unwrap_or(ready)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_panel_load_is_complete_and_read_decode_refusals_preserve_state() {
        let state = Publication::default();
        let mut program = crate::Program::default();
        program.project.settings.name = "Panel recall".into();
        let encoded = program.encode().unwrap();
        load_reader(&state, std::io::Cursor::new(&encoded)).unwrap();
        assert_eq!(state.capture().unwrap(), encoded);
        assert!(load_reader(&state, std::io::Cursor::new(b"corrupt packet")).is_err());
        struct Failed;
        impl std::io::Read for Failed {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("injected read refusal"))
            }
        }
        assert!(
            load_reader(&state, Failed)
                .unwrap_err()
                .contains("injected read refusal")
        );
        assert_eq!(state.capture().unwrap(), encoded);
    }
    #[test]
    fn the_panel_words_follow_the_language() {
        let english = Words(Language::English);
        let spanish = Words(Language::Spanish);
        assert_eq!(english.load(), "Load program");
        assert_eq!(spanish.load(), "Cargar programa");
        assert_eq!(spanish.title(), "Windfall — 16 salidas estéreo");
        // A refusal to load is said in the panel's language too.
        let state = Publication::default();
        let refused = load_program(&state, "", spanish).unwrap_err();
        assert!(!refused.is_empty());
        let refused = load_reader(&state, std::io::Cursor::new(b"corrupt packet"))
            .map_err(|error| spanish.say(&error))
            .unwrap_err();
        assert!(!refused.contains("windfall-message:"), "{refused}");
    }
}
#[cfg(windows)]
mod windows {
    use super::*;
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::*, Graphics::Gdi::COLOR_WINDOW, System::LibraryLoader::*,
        UI::WindowsAndMessaging::*,
    };
    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }
    struct Context {
        state: Rc<Publication>,
        words: Words,
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
                let label = context.words.loaded(&context.state, &path);
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
            Self::open_in(parent, state, Words::chosen())
        }
        fn open_in(
            parent: *mut c_void,
            state: Rc<Publication>,
            words: Words,
        ) -> Result<Self, String> {
            if parent.is_null() {
                return Err("Missing host editor parent".into());
            }
            let mut context = Box::new(Context {
                state,
                words,
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
            child("STATIC", &words.title(), 0, 12, 12, 530, 24, 0);
            child("STATIC", &words.state_file(), 0, 12, 44, 530, 20, 0);
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
            child("BUTTON", &words.load(), WS_TABSTOP, 436, 68, 112, 26, 1);
            context.status = child(
                "STATIC",
                &status_text(&context.state, words.host_first(), words),
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
    #[cfg(test)]
    mod tests {
        use super::*;
        /// The text of every control of `window`, in creation order.
        fn texts(window: HWND) -> Vec<String> {
            let mut texts = Vec::new();
            let mut child = unsafe { GetWindow(window, GW_CHILD) };
            while !child.is_null() {
                let mut text = [0u16; 512];
                let size = unsafe { GetWindowTextW(child, text.as_mut_ptr(), text.len() as i32) };
                texts.push(String::from_utf16_lossy(&text[..size.max(0) as usize]));
                child = unsafe { GetWindow(child, GW_HWNDNEXT) };
            }
            texts
        }
        #[test]
        fn the_native_panel_renders_in_spanish_and_in_english() {
            // A hidden top-level window stands for the host's editor frame.
            let host = unsafe {
                CreateWindowExW(
                    0,
                    wide("STATIC").as_ptr(),
                    wide("host").as_ptr(),
                    WS_OVERLAPPED,
                    0,
                    0,
                    600,
                    200,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null(),
                )
            };
            assert!(!host.is_null());
            for (language, expected) in [
                (
                    Language::Spanish,
                    [
                        "Windfall — 16 salidas estéreo",
                        "Archivo de estado del programa exportado (windfall-state.json):",
                        "",
                        "Cargar programa",
                        "El transporte, el tempo y el compás del host tienen prioridad sobre el documento.",
                    ],
                ),
                (
                    Language::English,
                    [
                        "Windfall — 16 stereo outputs",
                        "Exported program state file (windfall-state.json):",
                        "",
                        "Load program",
                        "Host transport, tempo and meter take precedence over the document.",
                    ],
                ),
            ] {
                let panel =
                    Panel::open_in(host, Rc::new(Publication::default()), Words(language)).unwrap();
                assert_eq!(texts(panel.window), expected, "{language:?}");
                drop(panel);
            }
            unsafe {
                DestroyWindow(host);
            }
        }
    }
}
