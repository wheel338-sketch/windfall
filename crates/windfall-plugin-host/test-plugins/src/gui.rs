//! The gain plugin's editor on Windows: one child window with nothing in
//! it, which is all a host needs to embed, size and close.
//!
//! Opening it registers a 30 ms timer with the host and asks for a main
//! thread callback. On the timer's third tick the plugin asks the host to
//! make the window 400 by 240, the way a plugin does when the user folds
//! out a panel.

use std::ffi::{CStr, c_char, c_void};
use std::ptr;

use clap_sys::ext::gui::{
    CLAP_EXT_GUI, CLAP_WINDOW_API_WIN32, clap_gui_resize_hints, clap_host_gui, clap_plugin_gui,
    clap_window,
};
use clap_sys::ext::timer_support::{CLAP_EXT_TIMER_SUPPORT, clap_host_timer_support};
use clap_sys::plugin::clap_plugin;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, MoveWindow, SW_HIDE, SW_SHOW, ShowWindow, WS_CHILD,
};
use windows_sys::core::w;

use crate::plugin::Plugin;

const MIN_WIDTH: u32 = 160;
const MIN_HEIGHT: u32 = 100;
/// The editor's size is always a multiple of this in both directions.
const GRID: u32 = 20;
const REQUESTED_SIZE: (u32, u32) = (400, 240);
const RESIZE_ON_TICK: u64 = 3;

/// # Safety
/// `plugin.host` must still be the valid host the plugin was created with.
unsafe fn host_extension<T>(plugin: &Plugin, id: &CStr) -> *const T {
    // SAFETY: the host outlives the plugin.
    match unsafe { (*plugin.host).get_extension } {
        // SAFETY: as above, and the id is a valid C string.
        Some(get_extension) => unsafe { get_extension(plugin.host, id.as_ptr()) as *const T },
        None => ptr::null(),
    }
}

fn snap(width: u32, height: u32) -> (u32, u32) {
    (
        (width / GRID * GRID).max(MIN_WIDTH),
        (height / GRID * GRID).max(MIN_HEIGHT),
    )
}

unsafe extern "C" fn is_api_supported(
    _plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    // SAFETY: the host passes a valid C string.
    !is_floating && !api.is_null() && unsafe { CStr::from_ptr(api) } == CLAP_WINDOW_API_WIN32
}

unsafe extern "C" fn get_preferred_api(
    _plugin: *const clap_plugin,
    api: *mut *const c_char,
    is_floating: *mut bool,
) -> bool {
    // SAFETY: the host passes room for both answers.
    unsafe {
        *api = CLAP_WINDOW_API_WIN32.as_ptr();
        *is_floating = false;
    }
    true
}

unsafe extern "C" fn create(
    plugin: *const clap_plugin,
    api: *const c_char,
    is_floating: bool,
) -> bool {
    // SAFETY: the arguments are the host's.
    if !unsafe { is_api_supported(plugin, api, is_floating) } {
        return false;
    }
    // SAFETY: the host passes the plugin it was given, on the main thread.
    unsafe {
        let plugin = Plugin::from_raw(plugin);
        let main = plugin.main();
        if main.gui_created {
            return false;
        }
        main.gui_created = true;
        main.width = 320;
        main.height = 200;
        main.timer_ticks = 0;

        let timers: *const clap_host_timer_support = host_extension(plugin, CLAP_EXT_TIMER_SUPPORT);
        if !timers.is_null()
            && let Some(register_timer) = (*timers).register_timer
        {
            let mut id = 0;
            if register_timer(plugin.host, 30, &mut id) {
                main.timer = Some(id);
            }
        }
        if let Some(request_callback) = (*plugin.host).request_callback {
            request_callback(plugin.host);
        }
    }
    true
}

/// Destroys the editor's window, if it has one.
///
/// # Safety
/// Main thread only.
pub(crate) unsafe fn destroy_window(plugin: &Plugin) {
    // SAFETY: this is the main thread by the caller's promise.
    let main = unsafe { plugin.main() };
    if !main.window.is_null() {
        // SAFETY: the window is the one made in `set_parent`.
        unsafe { DestroyWindow(main.window) };
        main.window = ptr::null_mut();
    }
}

unsafe extern "C" fn destroy(plugin: *const clap_plugin) {
    // SAFETY: the host passes the plugin it was given, on the main thread.
    unsafe {
        let plugin = Plugin::from_raw(plugin);
        destroy_window(plugin);
        let main = plugin.main();
        if let Some(timer) = main.timer.take() {
            let timers: *const clap_host_timer_support =
                host_extension(plugin, CLAP_EXT_TIMER_SUPPORT);
            if !timers.is_null()
                && let Some(unregister_timer) = (*timers).unregister_timer
            {
                unregister_timer(plugin.host, timer);
            }
        }
        main.gui_created = false;
    }
}

unsafe extern "C" fn set_scale(plugin: *const clap_plugin, scale: f64) -> bool {
    if !scale.is_finite() || !(0.1..=16.0).contains(&scale) {
        return false;
    }
    // SAFETY: normal CLAP gui main-thread call with the live fixture instance.
    unsafe {
        let plugin = Plugin::from_raw(plugin);
        // Existing Invert mode also injects GUI scale refusal for this fixture.
        if plugin.value(crate::plugin::slot::MODE) >= 0.5 {
            return false;
        }
        let (width, height) = snap(
            (320.0 * scale).round() as u32,
            (200.0 * scale).round() as u32,
        );
        apply_size(plugin, width, height);
    }
    true
}

unsafe extern "C" fn get_size(
    plugin: *const clap_plugin,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    // SAFETY: the host passes the plugin it was given and room for a size.
    unsafe {
        let main = Plugin::from_raw(plugin).main();
        *width = main.width;
        *height = main.height;
    }
    true
}

unsafe extern "C" fn can_resize(_plugin: *const clap_plugin) -> bool {
    true
}

unsafe extern "C" fn get_resize_hints(
    _plugin: *const clap_plugin,
    hints: *mut clap_gui_resize_hints,
) -> bool {
    // SAFETY: the host passes room for the hints.
    unsafe {
        *hints = clap_gui_resize_hints {
            can_resize_horizontally: true,
            can_resize_vertically: true,
            preserve_aspect_ratio: false,
            aspect_ratio_width: 1,
            aspect_ratio_height: 1,
        };
    }
    true
}

unsafe extern "C" fn adjust_size(
    _plugin: *const clap_plugin,
    width: *mut u32,
    height: *mut u32,
) -> bool {
    // SAFETY: the host passes a size to read and write.
    unsafe { (*width, *height) = snap(*width, *height) };
    true
}

/// # Safety
/// Main thread only.
unsafe fn apply_size(plugin: &Plugin, width: u32, height: u32) {
    // SAFETY: this is the main thread by the caller's promise.
    let main = unsafe { plugin.main() };
    main.width = width;
    main.height = height;
    if !main.window.is_null() {
        // SAFETY: the window is the one made in `set_parent`.
        unsafe { MoveWindow(main.window, 0, 0, width as i32, height as i32, 1) };
    }
}

unsafe extern "C" fn set_size(plugin: *const clap_plugin, width: u32, height: u32) -> bool {
    if snap(width, height) != (width, height) {
        return false;
    }
    // SAFETY: the host passes the plugin it was given, on the main thread.
    unsafe { apply_size(Plugin::from_raw(plugin), width, height) };
    true
}

unsafe extern "C" fn set_parent(plugin: *const clap_plugin, window: *const clap_window) -> bool {
    // SAFETY: the host passes the plugin it was given and a valid window.
    unsafe {
        let plugin = Plugin::from_raw(plugin);
        let main = plugin.main();
        if !main.gui_created || !main.window.is_null() || window.is_null() {
            return false;
        }
        if CStr::from_ptr((*window).api) != CLAP_WINDOW_API_WIN32 {
            return false;
        }
        main.window = CreateWindowExW(
            0,
            w!("STATIC"),
            w!("Windfall test gain"),
            WS_CHILD,
            0,
            0,
            main.width as i32,
            main.height as i32,
            (*window).specific.win32,
            ptr::null_mut(),
            GetModuleHandleW(ptr::null()),
            ptr::null(),
        );
        !main.window.is_null()
    }
}

unsafe extern "C" fn set_transient(
    _plugin: *const clap_plugin,
    _window: *const clap_window,
) -> bool {
    false
}

unsafe extern "C" fn suggest_title(_plugin: *const clap_plugin, _title: *const c_char) {}

/// # Safety
/// `plugin` must be the host's valid plugin, on the main thread.
unsafe fn set_visible(plugin: *const clap_plugin, visible: bool) -> bool {
    // SAFETY: by the caller's promise.
    let main = unsafe { Plugin::from_raw(plugin).main() };
    if main.window.is_null() {
        return false;
    }
    // SAFETY: the window is the one made in `set_parent`.
    unsafe { ShowWindow(main.window, if visible { SW_SHOW } else { SW_HIDE }) };
    true
}

unsafe extern "C" fn show(plugin: *const clap_plugin) -> bool {
    // SAFETY: the host passes the plugin it was given, on the main thread.
    unsafe { set_visible(plugin, true) }
}

unsafe extern "C" fn hide(plugin: *const clap_plugin) -> bool {
    // SAFETY: the host passes the plugin it was given, on the main thread.
    unsafe { set_visible(plugin, false) }
}

/// Asks the host for a larger window once, a few ticks after opening.
///
/// # Safety
/// Main thread only.
pub(crate) unsafe fn on_timer(plugin: &Plugin) {
    // SAFETY: this is the main thread by the caller's promise.
    unsafe {
        let main = plugin.main();
        if !main.gui_created || main.timer_ticks != RESIZE_ON_TICK {
            return;
        }
        let gui: *const clap_host_gui = host_extension(plugin, CLAP_EXT_GUI);
        if gui.is_null() {
            return;
        }
        let Some(request_resize) = (*gui).request_resize else {
            return;
        };
        let (width, height) = REQUESTED_SIZE;
        if request_resize(plugin.host, width, height) {
            apply_size(plugin, width, height);
        }
    }
}

static GUI: clap_plugin_gui = clap_plugin_gui {
    is_api_supported: Some(is_api_supported),
    get_preferred_api: Some(get_preferred_api),
    create: Some(create),
    destroy: Some(destroy),
    set_scale: Some(set_scale),
    get_size: Some(get_size),
    can_resize: Some(can_resize),
    get_resize_hints: Some(get_resize_hints),
    adjust_size: Some(adjust_size),
    set_size: Some(set_size),
    set_parent: Some(set_parent),
    set_transient: Some(set_transient),
    suggest_title: Some(suggest_title),
    show: Some(show),
    hide: Some(hide),
};

pub(crate) fn extension(id: &CStr) -> *const c_void {
    if id == CLAP_EXT_GUI {
        &raw const GUI as *const c_void
    } else {
        ptr::null()
    }
}
