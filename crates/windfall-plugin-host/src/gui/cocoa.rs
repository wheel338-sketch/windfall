//! Owned NSWindow plus an NSView parent. All coordinates are AppKit points.
use super::{
    cocoa_ffi::*,
    lifecycle::{Backend, Observation},
};
use std::{cell::Cell, ffi::CString, ptr, sync::OnceLock};
unsafe extern "C" fn should_close(delegate: Obj, _: Obj, _: Obj) -> i8 {
    let mut value = ptr::null_mut();
    unsafe {
        object_getInstanceVariable(delegate, c"windfallClose".as_ptr(), &mut value);
    }
    if let Some(close) = unsafe { value.cast::<Cell<bool>>().as_ref() } {
        close.set(true);
    }
    // Defer closing until idle has removed/destroyed the plugin's GUI.
    0
}
struct ClassLease(Obj);
impl Drop for ClassLease {
    fn drop(&mut self) {
        unsafe {
            objc_disposeClassPair(self.0);
        }
    }
}
fn delegate_class(close: &Cell<bool>) -> Result<ClassLease, String> {
    let name = CString::new(format!("WindfallHostedEditorDelegate_{close:p}")).unwrap();
    let class = unsafe {
        let class = objc_allocateClassPair(objc_getClass(c"NSObject".as_ptr()), name.as_ptr(), 0);
        if class.is_null() {
            return Err("Cocoa editor delegate allocation failed".into());
        }
        let implementation: unsafe extern "C" fn() =
            std::mem::transmute(should_close as unsafe extern "C" fn(Obj, Obj, Obj) -> i8);
        let encoding = if cfg!(target_arch = "aarch64") {
            c"B@:@"
        } else {
            c"c@:@"
        };
        if class_addIvar(
            class,
            c"windfallClose".as_ptr(),
            size_of::<*mut Cell<bool>>(),
            align_of::<*mut Cell<bool>>().trailing_zeros() as u8,
            c"^v".as_ptr(),
        ) == 0
            || class_addMethod(
                class,
                selector(c"windowShouldClose:"),
                implementation,
                encoding.as_ptr(),
            ) == 0
        {
            objc_disposeClassPair(class);
            return Err("Cocoa editor delegate registration failed".into());
        }
        objc_registerClassPair(class);
        class
    };
    Ok(ClassLease(class))
}
pub(crate) struct Window {
    window: Obj,
    view: Obj,
    delegate: Obj,
    _class: ClassLease,
    close: Box<Cell<bool>>,
    application: Cell<f64>,
}
impl Window {
    pub fn create(
        title: &str,
        width: u32,
        height: u32,
        resizable: bool,
        position: Option<(i32, i32)>,
    ) -> Result<Self, String> {
        if !is_main() {
            return Err("Cocoa editors require the application main thread".into());
        }
        let _pool = Pool::new();
        let mut close = Box::new(Cell::new(false));
        let class = delegate_class(&close)?;
        unsafe {
            let application = send(
                objc_getClass(c"NSApplication".as_ptr()),
                c"sharedApplication",
            );
            // The helper has no NSApplication.run. Finish its launch once;
            // Tauri's already running application must not receive it again.
            static LAUNCHED: OnceLock<()> = OnceLock::new();
            if !bool_result(application, c"isRunning") && LAUNCHED.get().is_none() {
                void(application, c"finishLaunching");
                let _ = LAUNCHED.set(());
            }
            let initialize: unsafe extern "C" fn(Obj, Obj, Rect, usize, usize, i8) -> Obj =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let window = initialize(
                send(objc_getClass(c"NSWindow".as_ptr()), c"alloc"),
                selector(c"initWithContentRect:styleMask:backing:defer:"),
                Rect {
                    origin: Point {
                        x: position.map_or(100.0, |p| f64::from(p.0)),
                        y: position.map_or(100.0, |p| f64::from(p.1)),
                    },
                    size: Size {
                        width: f64::from(width),
                        height: f64::from(height),
                    },
                },
                1 | 2 | 4 | if resizable { 8 } else { 0 },
                2,
                0,
            );
            if window.is_null() {
                return Err("AppKit refused the editor window".into());
            }
            boolean(window, c"setReleasedWhenClosed:", false);
            let text = string(title);
            object(window, c"setTitle:", text);
            void(text, c"release");
            let view = send(window, c"contentView");
            if view.is_null() {
                void(window, c"release");
                return Err("AppKit did not create a parent NSView".into());
            }
            let delegate = send(send(class.0, c"alloc"), c"init");
            if delegate.is_null() {
                void(window, c"release");
                return Err("Cocoa editor delegate allocation failed".into());
            }
            object_setInstanceVariable(
                delegate,
                c"windfallClose".as_ptr(),
                (&mut *close as *mut Cell<bool>).cast(),
            );
            object(window, c"setDelegate:", delegate);
            Ok(Self {
                window,
                view,
                delegate,
                _class: class,
                close,
                application: Cell::new(1.0),
            })
        }
    }
    pub fn handle(&self) -> Obj {
        self.view
    }
    pub fn set_application_scale(&self, application: f64) {
        let logical = self.observe().size;
        self.application.set(application);
        self.resize(logical);
    }
    pub fn show(&self, focus: bool) {
        unsafe {
            object(
                self.window,
                if focus {
                    c"makeKeyAndOrderFront:"
                } else {
                    c"orderFront:"
                },
                ptr::null_mut(),
            );
        }
    }
    pub fn set_owner(&self, owner: Option<usize>) -> Result<(), String> {
        if let Some(owner) = owner {
            if owner == 0 {
                return Err("Missing Cocoa owner NSWindow".into());
            }
            let call: unsafe extern "C" fn(Obj, Obj, Obj, isize) =
                unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
            unsafe {
                call(
                    owner as Obj,
                    selector(c"addChildWindow:ordered:"),
                    self.window,
                    1,
                );
            }
        }
        Ok(())
    }
}
impl Backend for Window {
    fn observe(&self) -> Observation {
        unsafe {
            let size = rect(self.view, c"bounds").size;
            let screen = send(self.window, c"screen");
            let monitor = if screen.is_null() {
                1.0
            } else {
                double_result(screen, c"backingScaleFactor")
            };
            Observation {
                size: (
                    size.width.round().clamp(1.0, 32768.0) as u32,
                    size.height.round().clamp(1.0, 32768.0) as u32,
                ),
                monitor,
                closed: self.close.get(),
            }
        }
    }
    fn resize(&self, size: (u32, u32)) {
        let call: unsafe extern "C" fn(Obj, Obj, Size) =
            unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
        unsafe {
            call(
                self.window,
                selector(c"setContentSize:"),
                Size {
                    width: f64::from(size.0) * self.application.get(),
                    height: f64::from(size.1) * self.application.get(),
                },
            );
            call(
                self.view,
                selector(c"setBoundsSize:"),
                Size {
                    width: f64::from(size.0),
                    height: f64::from(size.1),
                },
            );
        }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        debug_assert!(is_main());
        unsafe {
            object(self.window, c"setDelegate:", ptr::null_mut());
            let parent = send(self.window, c"parentWindow");
            if !parent.is_null() {
                object(parent, c"removeChildWindow:", self.window);
            }
            void(self.window, c"close");
            void(self.window, c"release");
            void(self.delegate, c"release");
        }
    }
}
pub(crate) fn pump_events() {
    if !is_main() {
        return;
    }
    let _pool = Pool::new();
    unsafe {
        let app = send(
            objc_getClass(c"NSApplication".as_ptr()),
            c"sharedApplication",
        );
        // Tauri owns NSApplication.run on the desktop. Do not recursively
        // dispatch application events while its plugin owner map is borrowed.
        // The helper has only finishLaunching and uses this explicit pump.
        if bool_result(app, c"isRunning") {
            return;
        }
        let mode = string("kCFRunLoopDefaultMode");
        let date = send(objc_getClass(c"NSDate".as_ptr()), c"distantPast");
        let next: unsafe extern "C" fn(Obj, Obj, usize, Obj, Obj, i8) -> Obj =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        // Bound work even when a plugin continually posts events. No wait on
        // the audio callback: this function is exclusively GUI-owner control.
        for _ in 0..256 {
            let event = next(
                app,
                selector(c"nextEventMatchingMask:untilDate:inMode:dequeue:"),
                usize::MAX,
                date,
                mode,
                1,
            );
            if event.is_null() {
                break;
            }
            object(app, c"sendEvent:", event);
        }
        void(app, c"updateWindows");
        void(mode, c"release");
    }
}
