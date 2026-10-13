//! Cocoa/X11 adapter with owner-local polling; no native callback owns a plugin.
#[cfg(target_os = "macos")]
use super::cocoa::Window;
use super::lifecycle::{Backend, Lifecycle, LocalRegistry};
#[cfg(target_os = "linux")]
use super::x11::Window;
use std::{cell::RefCell, ffi::c_void, rc::Rc};

pub(crate) trait WindowClient {
    fn adjust_size(&self, width: u32, height: u32) -> (u32, u32);
    fn resized(&self, width: u32, height: u32);
    fn accepted_size(&self) -> (u32, u32);
}
thread_local! {
    static TOKEN: u8 = const { 0 };
    // A destructor-bearing TLS vector would register code for thread exit. That
    // code may already be unmapped when this host is linked into an export DLL.
    static WINDOWS: LocalRegistry<State> = const { LocalRegistry::new() };
}
pub(crate) fn token() -> usize {
    TOKEN.with(|v| std::ptr::from_ref(v) as usize)
}
struct State {
    // Client COM references retire before the parent NSView/XID. Editors have
    // already removed their plugin view and cleared its frame before this Drop.
    client: Box<dyn WindowClient>,
    native: Window,
    lifecycle: RefCell<Lifecycle>,
}
impl State {
    fn poll(&self) {
        let changed = self
            .lifecycle
            .borrow_mut()
            .poll(token(), &self.native, |w, h| self.client.adjust_size(w, h));
        // Release the lifecycle borrow before entering the plugin (which may
        // synchronously request another size from its frame).
        if let Some((w, h)) = changed {
            self.client.resized(w, h);
            let accepted = self.client.accepted_size();
            if accepted != (w, h) {
                self.lifecycle
                    .borrow_mut()
                    .resize(token(), &self.native, accepted);
            }
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        WINDOWS.with(LocalRegistry::retire);
    }
}
pub(crate) struct HostWindow {
    state: Rc<State>,
}
impl HostWindow {
    pub fn create(
        title: &str,
        width: u32,
        height: u32,
        resizable: bool,
        position: Option<(i32, i32)>,
        client: Box<dyn WindowClient>,
    ) -> Result<Self, String> {
        super::lifecycle::dimensions(width, height).ok_or("invalid editor dimensions")?;
        let native = Window::create(title, width, height, resizable, position)?;
        let lifecycle = Lifecycle::new(token(), (width, height), native.observe().monitor)
            .map_err(|e| e.to_string())?;
        let state = Rc::new(State {
            native,
            lifecycle: RefCell::new(lifecycle),
            client,
        });
        WINDOWS.with(|registry| registry.add(&state));
        Ok(Self { state })
    }
    /// NSView* on Cocoa; XID encoded in the pointer-sized value on X11.
    pub fn handle(&self) -> *mut c_void {
        self.state.native.handle()
    }
    #[cfg(target_os = "macos")]
    pub fn set_application_scale(&self, application: f64) -> Result<(), super::EditorError> {
        super::scaling::content_scale(application, self.monitor_scale())?;
        self.state.native.set_application_scale(application);
        Ok(())
    }
    pub fn show(&self, focus: bool) {
        self.state.native.show(focus);
    }
    pub fn set_owner(&self, owner: Option<usize>) -> Result<(), String> {
        self.state.native.set_owner(owner)
    }
    pub fn set_client_size(&self, width: u32, height: u32) {
        self.state
            .lifecycle
            .borrow_mut()
            .resize(token(), &self.state.native, (width, height));
    }
    pub fn monitor_scale(&self) -> f64 {
        self.state.lifecycle.borrow().monitor()
    }
    pub fn take_monitor_scale(&self) -> Option<f64> {
        self.state.poll();
        self.state.lifecycle.borrow_mut().take_monitor()
    }
    pub fn take_close_request(&self) -> bool {
        self.state.poll();
        self.state.lifecycle.borrow().closing()
    }
}
pub(crate) fn pump_events(wait: Option<std::time::Duration>) {
    #[cfg(target_os = "macos")]
    super::cocoa::pump_events();
    let windows = WINDOWS.with(LocalRegistry::live);
    for window in windows {
        window.poll();
    }
    if let Some(wait) = wait.filter(|wait| !wait.is_zero()) {
        std::thread::sleep(wait);
    }
}
