use super::*;
use clap_sys::ext::gui::{CLAP_EXT_GUI, clap_plugin_gui, clap_window};
#[cfg(target_os = "linux")]
use clap_sys::ext::timer_support::{
    CLAP_EXT_TIMER_SUPPORT, clap_host_timer_support, clap_plugin_timer_support,
};
#[derive(Default)]
pub(super) struct Editor {
    created: bool,
    panel: Option<crate::panel::Panel>,
    #[cfg(target_os = "linux")]
    timer: Option<u32>,
}
unsafe extern "C" fn supported(_: *const clap_plugin, api: *const c_char, floating: bool) -> bool {
    !floating && crate::panel::supports(api)
}
unsafe extern "C" fn preferred(
    _: *const clap_plugin,
    api: *mut *const c_char,
    floating: *mut bool,
) -> bool {
    if !cfg!(any(windows, target_os = "macos", target_os = "linux"))
        || api.is_null()
        || floating.is_null()
    {
        return false;
    }
    unsafe {
        #[cfg(windows)]
        api.write(c"win32".as_ptr());
        #[cfg(target_os = "macos")]
        api.write(c"cocoa".as_ptr());
        #[cfg(target_os = "linux")]
        api.write(c"x11".as_ptr());
        floating.write(false);
    }
    true
}
unsafe extern "C" fn create(
    plugin: *const clap_plugin,
    api: *const c_char,
    floating: bool,
) -> bool {
    let Some(instance) = (unsafe { Instance::get(plugin) }) else {
        return false;
    };
    let editor = unsafe { &mut *instance.gui.get() };
    if editor.created || !unsafe { supported(plugin, api, floating) } {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        let Some(timer) = (unsafe { host_timer(instance) }) else {
            return false;
        };
        let mut id = u32::MAX;
        if !timer
            .register_timer
            .is_some_and(|register| unsafe { register(instance.host, 16, &mut id) })
        {
            return false;
        }
        editor.timer = Some(id);
    }
    editor.created = true;
    true
}
pub(super) unsafe extern "C" fn destroy(plugin: *const clap_plugin) {
    if let Some(instance) = unsafe { Instance::get(plugin) } {
        #[cfg(target_os = "linux")]
        if let Some(id) = (unsafe { &mut *instance.gui.get() }).timer.take()
            && let Some(timer) = (unsafe { host_timer(instance) })
            && let Some(unregister) = timer.unregister_timer
        {
            unsafe {
                unregister(instance.host, id);
            }
        }
        unsafe {
            *instance.gui.get() = Editor::default();
        }
    }
}
unsafe extern "C" fn size(_: *const clap_plugin, width: *mut u32, height: *mut u32) -> bool {
    if width.is_null() || height.is_null() {
        return false;
    }
    unsafe {
        width.write(560);
        height.write(180);
    }
    true
}
unsafe extern "C" fn parent(plugin: *const clap_plugin, window: *const clap_window) -> bool {
    let Some(instance) = (unsafe { Instance::get(plugin) }) else {
        return false;
    };
    let Some(window) = (unsafe { window.as_ref() }) else {
        return false;
    };
    let editor = unsafe { &mut *instance.gui.get() };
    if !editor.created || editor.panel.is_some() || !crate::panel::supports(window.api) {
        return false;
    }
    #[cfg(windows)]
    let parent = unsafe { window.specific.win32 };
    #[cfg(target_os = "macos")]
    let parent = unsafe { window.specific.cocoa };
    #[cfg(target_os = "linux")]
    let parent = unsafe { window.specific.x11 } as usize as *mut c_void;
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    let parent = ptr::null_mut();
    match crate::panel::Panel::open(parent, Rc::clone(&instance.state)) {
        Ok(panel) => {
            editor.panel = Some(panel);
            true
        }
        Err(_) => false,
    }
}
unsafe extern "C" fn show(plugin: *const clap_plugin) -> bool {
    unsafe { visible(plugin, true) }
}
unsafe extern "C" fn hide(plugin: *const clap_plugin) -> bool {
    unsafe { visible(plugin, false) }
}
unsafe fn visible(plugin: *const clap_plugin, value: bool) -> bool {
    let Some(instance) = (unsafe { Instance::get(plugin) }) else {
        return false;
    };
    let Some(panel) = (unsafe { &*instance.gui.get() }).panel.as_ref() else {
        return false;
    };
    panel.visible(value);
    true
}
unsafe extern "C" fn scale(_: *const clap_plugin, value: f64) -> bool {
    value == 1.0
}
unsafe extern "C" fn resize(_: *const clap_plugin) -> bool {
    false
}
static GUI: clap_plugin_gui = clap_plugin_gui {
    is_api_supported: Some(supported),
    get_preferred_api: Some(preferred),
    create: Some(create),
    destroy: Some(destroy),
    set_scale: Some(scale),
    get_size: Some(size),
    can_resize: Some(resize),
    get_resize_hints: None,
    adjust_size: None,
    set_size: None,
    set_parent: Some(parent),
    set_transient: None,
    suggest_title: None,
    show: Some(show),
    hide: Some(hide),
};
pub(super) fn extension(id: &CStr) -> *const c_void {
    #[cfg(target_os = "linux")]
    if id == CLAP_EXT_TIMER_SUPPORT {
        return (&raw const TIMER).cast();
    }
    if id == CLAP_EXT_GUI {
        (&raw const GUI).cast()
    } else {
        ptr::null()
    }
}
#[cfg(target_os = "linux")]
unsafe fn host_timer(instance: &Instance) -> Option<&clap_host_timer_support> {
    let get = unsafe { (*instance.host).get_extension }?;
    unsafe {
        get(instance.host, CLAP_EXT_TIMER_SUPPORT.as_ptr())
            .cast::<clap_host_timer_support>()
            .as_ref()
    }
}
#[cfg(target_os = "linux")]
unsafe extern "C" fn on_timer(plugin: *const clap_plugin, id: u32) {
    let Some(instance) = (unsafe { Instance::get(plugin) }) else {
        return;
    };
    let editor = unsafe { &*instance.gui.get() };
    if editor.timer == Some(id)
        && let Some(panel) = &editor.panel
    {
        panel.poll();
    }
}
#[cfg(target_os = "linux")]
static TIMER: clap_plugin_timer_support = clap_plugin_timer_support {
    on_timer: Some(on_timer),
};
