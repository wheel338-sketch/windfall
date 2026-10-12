//! The host's window around a plugin editor, on Windows.
//!
//! All the Win32 calls of the crate are in this file. A [`HostWindow`] is a
//! plain top-level window with no content of its own: the plugin puts a
//! child window in it. The window does two things besides existing. It asks
//! the plugin what sizes it accepts while the user drags the frame, and it
//! remembers that the user asked to close it, which the instance acts on
//! in its next `idle`.

use std::cell::Cell;
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::OnceLock;
use std::time::Duration;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{BLACK_BRUSH, GetStockObject, HBRUSH};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CREATESTRUCTW, CW_USEDEFAULT, CreateWindowExW, DefWindowProcW,
    DestroyWindow, DispatchMessageW, GWLP_HWNDPARENT, GWLP_USERDATA, GetWindowLongPtrW, IDC_ARROW,
    IsWindow, LoadCursorW, MSG, MsgWaitForMultipleObjects, PM_REMOVE, PeekMessageW, QS_ALLINPUT,
    RegisterClassExW, SIZE_MINIMIZED, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage, WM_CLOSE,
    WM_NCCREATE, WM_SIZE, WM_SIZING, WMSZ_BOTTOMLEFT, WMSZ_LEFT, WMSZ_TOP, WMSZ_TOPLEFT,
    WMSZ_TOPRIGHT, WNDCLASSEXW, WS_CAPTION, WS_CLIPCHILDREN, WS_MAXIMIZEBOX, WS_MINIMIZEBOX,
    WS_OVERLAPPED, WS_SYSMENU, WS_THICKFRAME,
};
use windows_sys::core::w;

/// What a host window asks of the editor inside it. Both are called from
/// the window procedure, on the thread that owns the window.
pub(crate) trait WindowClient {
    /// The user is dragging the frame to this content size. Returns the
    /// nearest size the editor can take.
    fn adjust_size(&self, width: u32, height: u32) -> (u32, u32);
    /// The content area now has this size, because the user resized the
    /// window.
    fn resized(&self, width: u32, height: u32);
}

struct WindowState {
    client: Box<dyn WindowClient>,
    close_requested: Cell<bool>,
    /// Set while the host itself resizes the window, so the editor is not
    /// told about a size it asked for.
    quiet: Cell<bool>,
    style: u32,
}

/// A top-level window for one plugin editor. It is destroyed when dropped,
/// which has to happen on the thread that created it. The type is not
/// `Send`, so the compiler sees to that.
pub(crate) struct HostWindow {
    hwnd: HWND,
    state: *mut WindowState,
}

const CLASS_NAME: *const u16 = w!("WindfallPluginEditor");

/// Registers the window class once. Returns false if Windows refused.
fn register_class() -> bool {
    static REGISTERED: OnceLock<bool> = OnceLock::new();
    *REGISTERED.get_or_init(|| {
        // SAFETY: the class structure is complete and its strings are
        // static. The procedure it names lives as long as the program.
        unsafe {
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: Some(window_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: GetModuleHandleW(ptr::null()),
                hIcon: ptr::null_mut(),
                hCursor: LoadCursorW(ptr::null_mut(), IDC_ARROW),
                hbrBackground: GetStockObject(BLACK_BRUSH) as HBRUSH,
                lpszMenuName: ptr::null(),
                lpszClassName: CLASS_NAME,
                hIconSm: ptr::null_mut(),
            };
            RegisterClassExW(&class) != 0
        }
    })
}

/// The size of the whole window whose content area is `width` by `height`.
fn outer_size(style: u32, width: u32, height: u32) -> (i32, i32) {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: width as i32,
        bottom: height as i32,
    };
    // SAFETY: `rect` is a live RECT the call reads and writes.
    unsafe { AdjustWindowRectEx(&mut rect, style, 0, 0) };
    (rect.right - rect.left, rect.bottom - rect.top)
}

impl HostWindow {
    /// Creates a hidden window whose content area is `width` by `height`
    /// pixels.
    pub fn create(
        title: &str,
        width: u32,
        height: u32,
        resizable: bool,
        position: Option<(i32, i32)>,
        client: Box<dyn WindowClient>,
    ) -> Result<Self, String> {
        if !register_class() {
            return Err("Windows refused the window class".to_owned());
        }
        let mut style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
        if resizable {
            style |= WS_THICKFRAME | WS_MAXIMIZEBOX;
        }
        let state = Box::into_raw(Box::new(WindowState {
            client,
            close_requested: Cell::new(false),
            quiet: Cell::new(false),
            style,
        }));
        let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
        let (outer_width, outer_height) = outer_size(style, width, height);
        let (x, y) = position.unwrap_or((CW_USEDEFAULT, CW_USEDEFAULT));
        // SAFETY: the class is registered, the title is a terminated UTF-16
        // string, and `state` stays alive until after the window is
        // destroyed in `drop`.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                CLASS_NAME,
                title.as_ptr(),
                style,
                x,
                y,
                outer_width,
                outer_height,
                ptr::null_mut(),
                ptr::null_mut(),
                GetModuleHandleW(ptr::null()),
                state as *const c_void,
            )
        };
        if hwnd.is_null() {
            // SAFETY: the window was not created, so nothing else holds the
            // state made above.
            drop(unsafe { Box::from_raw(state) });
            return Err("Windows refused to create the window".to_owned());
        }
        Ok(Self { hwnd, state })
    }

    /// The window's `HWND`.
    pub fn handle(&self) -> *mut c_void {
        self.hwnd
    }

    fn state(&self) -> &WindowState {
        // SAFETY: the state lives until `drop`, and only shared references
        // to it are ever made.
        unsafe { &*self.state }
    }

    /// Shows the window, with or without taking the keyboard.
    pub fn show(&self, take_focus: bool) {
        let command = if take_focus {
            SW_SHOW
        } else {
            SW_SHOWNOACTIVATE
        };
        // SAFETY: the window is alive.
        unsafe { ShowWindow(self.hwnd, command) };
    }

    /// Set native ownership, preserving a top-level plugin window. A DAW-owned
    /// editor minimizes/closes with that owner; a detached editor has no owner.
    pub fn set_owner(&self, owner: Option<usize>) -> Result<(), String> {
        let owner = owner.map_or(ptr::null_mut(), |value| value as HWND);
        if !owner.is_null() && unsafe { IsWindow(owner) } == 0 {
            return Err("editor owner window is unavailable".into());
        }
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_HWNDPARENT, owner as isize);
        }
        Ok(())
    }

    /// Resizes the window so that its content area is `width` by `height`,
    /// without telling the editor.
    pub fn set_client_size(&self, width: u32, height: u32) {
        let state = self.state();
        let (outer_width, outer_height) = outer_size(state.style, width, height);
        state.quiet.set(true);
        // SAFETY: the window is alive.
        unsafe {
            SetWindowPos(
                self.hwnd,
                ptr::null_mut(),
                0,
                0,
                outer_width,
                outer_height,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        state.quiet.set(false);
    }

    /// True once after the user has asked to close the window.
    pub fn take_close_request(&self) -> bool {
        self.state().close_requested.replace(false)
    }
}

impl Drop for HostWindow {
    fn drop(&mut self) {
        // SAFETY: the window is alive and this is its thread, because the
        // type cannot leave it. The state is freed only after the window is
        // gone, so the window procedure never sees it dangling.
        unsafe {
            DestroyWindow(self.hwnd);
            drop(Box::from_raw(self.state));
        }
    }
}

/// The system's scale factor: 1.0 at 96 dots per inch.
pub(crate) fn system_scale() -> f64 {
    // SAFETY: the call takes no arguments and only reads system settings.
    f64::from(unsafe { GetDpiForSystem() }) / 96.0
}

pub(crate) fn window_scale(handle: HWND) -> f64 {
    // SAFETY: caller owns a live editor HWND on its GUI thread; read-only call.
    let dpi = unsafe { GetDpiForWindow(handle) };
    if dpi == 0 { system_scale() } else { f64::from(dpi) / 96.0 }
}

pub(crate) fn pump_events(wait: Option<Duration>) {
    // SAFETY: the calls only read and dispatch this thread's own messages,
    // and `message` is a live MSG.
    unsafe {
        if let Some(wait) = wait {
            let millis = u32::try_from(wait.as_millis()).unwrap_or(u32::MAX - 1);
            MsgWaitForMultipleObjects(0, ptr::null(), 0, millis, QS_ALLINPUT);
        }
        let mut message: MSG = std::mem::zeroed();
        while PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// Fits the rectangle of a frame being dragged to a size the editor takes.
fn snap_drag(state: &WindowState, edge: u32, rect: &mut RECT) {
    let (frame_width, frame_height) = outer_size(state.style, 0, 0);
    let width = (rect.right - rect.left - frame_width).max(1) as u32;
    let height = (rect.bottom - rect.top - frame_height).max(1) as u32;
    let (width, height) = state.client.adjust_size(width, height);
    let outer_width = width as i32 + frame_width;
    let outer_height = height as i32 + frame_height;
    // The edge being dragged moves. The opposite one stays where it is.
    if matches!(edge, WMSZ_LEFT | WMSZ_TOPLEFT | WMSZ_BOTTOMLEFT) {
        rect.left = rect.right - outer_width;
    } else {
        rect.right = rect.left + outer_width;
    }
    if matches!(edge, WMSZ_TOP | WMSZ_TOPLEFT | WMSZ_TOPRIGHT) {
        rect.top = rect.bottom - outer_height;
    } else {
        rect.bottom = rect.top + outer_height;
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        // SAFETY: for this message `lparam` points at the CREATESTRUCTW of
        // the `CreateWindowExW` call, whose parameter is the state.
        unsafe {
            let create = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
    }
    // SAFETY: the user data is null or the state set above, which outlives
    // the window.
    let state = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const WindowState).as_ref() };

    // A panic must not unwind into Windows.
    let handled = catch_unwind(AssertUnwindSafe(|| {
        let state = state?;
        match message {
            WM_CLOSE => {
                state.close_requested.set(true);
                Some(0)
            }
            WM_SIZING => {
                // SAFETY: for this message `lparam` points at the RECT the
                // frame is being dragged to, which the handler may change.
                let rect = unsafe { &mut *(lparam as *mut RECT) };
                snap_drag(state, wparam as u32, rect);
                Some(1)
            }
            WM_SIZE if wparam as u32 != SIZE_MINIMIZED && !state.quiet.get() => {
                let width = (lparam & 0xffff) as u32;
                let height = ((lparam >> 16) & 0xffff) as u32;
                state.client.resized(width, height);
                Some(0)
            }
            _ => None,
        }
    }));
    match handled {
        Ok(Some(result)) => result,
        // SAFETY: the arguments are the ones Windows passed in.
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}
