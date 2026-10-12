use super::*;
use clap_sys::ext::gui::{CLAP_EXT_GUI, CLAP_WINDOW_API_WIN32, clap_plugin_gui, clap_window};
#[derive(Default)]
pub(super) struct Editor { created: bool, panel: Option<crate::panel::Panel> }
unsafe extern "C" fn supported(_: *const clap_plugin, api: *const c_char, floating: bool) -> bool {
    !floating && crate::panel::supports(api)
}
unsafe extern "C" fn preferred(_: *const clap_plugin, api: *mut *const c_char, floating: *mut bool) -> bool {
    if !cfg!(windows) || api.is_null() || floating.is_null() { return false; }
    unsafe { api.write(CLAP_WINDOW_API_WIN32.as_ptr()); floating.write(false); } true
}
unsafe extern "C" fn create(plugin: *const clap_plugin, api: *const c_char, floating: bool) -> bool {
    let Some(instance) = (unsafe { Instance::get(plugin) }) else { return false; };
    let editor = unsafe { &mut *instance.gui.get() };
    if editor.created || !unsafe { supported(plugin, api, floating) } { return false; }
    editor.created = true; true
}
unsafe extern "C" fn destroy(plugin: *const clap_plugin) {
    if let Some(instance) = unsafe { Instance::get(plugin) } { unsafe { *instance.gui.get() = Editor::default(); } }
}
unsafe extern "C" fn size(_: *const clap_plugin, width: *mut u32, height: *mut u32) -> bool {
    if width.is_null() || height.is_null() { return false; }
    unsafe { width.write(560); height.write(180); } true
}
unsafe extern "C" fn parent(plugin: *const clap_plugin, window: *const clap_window) -> bool {
    let Some(instance) = (unsafe { Instance::get(plugin) }) else { return false; };
    let Some(window) = (unsafe { window.as_ref() }) else { return false; };
    let editor = unsafe { &mut *instance.gui.get() };
    if !editor.created || editor.panel.is_some() || !crate::panel::supports(window.api) { return false; }
    #[cfg(windows)] let parent = unsafe { window.specific.win32 };
    #[cfg(not(windows))] let parent = ptr::null_mut();
    match crate::panel::Panel::open(parent, Rc::clone(&instance.state)) {
        Ok(panel) => { editor.panel = Some(panel); true }, Err(_) => false,
    }
}
unsafe extern "C" fn show(plugin: *const clap_plugin) -> bool { unsafe { visible(plugin, true) } }
unsafe extern "C" fn hide(plugin: *const clap_plugin) -> bool { unsafe { visible(plugin, false) } }
unsafe fn visible(plugin: *const clap_plugin, value: bool) -> bool {
    let Some(instance) = (unsafe { Instance::get(plugin) }) else { return false; };
    let Some(panel) = (unsafe { &*instance.gui.get() }).panel.as_ref() else { return false; }; panel.visible(value); true
}
unsafe extern "C" fn scale(_: *const clap_plugin, value: f64) -> bool { value == 1.0 }
unsafe extern "C" fn resize(_: *const clap_plugin) -> bool { false }
static GUI: clap_plugin_gui = clap_plugin_gui {
    is_api_supported: Some(supported), get_preferred_api: Some(preferred), create: Some(create), destroy: Some(destroy),
    set_scale: Some(scale), get_size: Some(size), can_resize: Some(resize), get_resize_hints: None, adjust_size: None, set_size: None,
    set_parent: Some(parent), set_transient: None, suggest_title: None, show: Some(show), hide: Some(hide),
};
pub(super) fn extension(id: &CStr) -> *const c_void { if id == CLAP_EXT_GUI { (&raw const GUI).cast() } else { ptr::null() } }
