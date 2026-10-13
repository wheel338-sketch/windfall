//! NSView control panel supplied to another DAW; no NSApplication/window owner.
#[path = "../../../windfall-plugin-host/src/gui/cocoa_ffi.rs"]
#[allow(dead_code)]
mod ffi;
use super::*;
use ffi::*;
use std::{ffi::CString, ptr};
struct Context {
    state: Rc<Publication>,
    path: Obj,
    status: Obj,
}
pub struct Panel {
    view: Obj,
    target: Obj,
    class: Obj,
    context: Box<Context>,
}
unsafe extern "C" fn load(target: Obj, _: Obj, _: Obj) {
    // Never unwind through AppKit. The target ivar points at the panel's stable
    // box until controls are detached and target released on this main thread.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let mut value = ptr::null_mut();
        object_getInstanceVariable(target, c"windfallContext".as_ptr(), &mut value);
        let Some(context) = value.cast::<Context>().as_ref() else {
            return;
        };
        let text = send(context.path, c"stringValue");
        let bytes = send(text, c"UTF8String").cast::<c_char>();
        if bytes.is_null() {
            return;
        }
        let path = CStr::from_ptr(bytes).to_string_lossy();
        let label = super::load_program(&context.state, &path)
            .err()
            .unwrap_or_else(|| "Program queued. Host transport and tempo drive playback.".into());
        let text = string(&label);
        object(context.status, c"setStringValue:", text);
        void(text, c"release");
    }));
}
// Keep native frame coordinates explicit at the small AppKit boundary.
#[allow(clippy::too_many_arguments)]
unsafe fn control(
    class: &CStr,
    parent: Obj,
    text: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    editable: bool,
) -> Obj {
    let initialize: unsafe extern "C" fn(Obj, Obj, Rect) -> Obj =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    let control = unsafe {
        initialize(
            send(objc_getClass(class.as_ptr()), c"alloc"),
            selector(c"initWithFrame:"),
            Rect {
                origin: Point { x, y },
                size: Size { width, height },
            },
        )
    };
    if control.is_null() {
        return control;
    }
    unsafe {
        let label = string(text);
        object(
            control,
            if class == c"NSButton" {
                c"setTitle:"
            } else {
                c"setStringValue:"
            },
            label,
        );
        void(label, c"release");
        if class == c"NSTextField" {
            boolean(control, c"setEditable:", editable);
            boolean(control, c"setSelectable:", editable);
            boolean(control, c"setBezeled:", editable);
            boolean(control, c"setDrawsBackground:", editable);
        }
        object(parent, c"addSubview:", control);
        void(control, c"release"); // retained by parent
    }
    control
}
impl Panel {
    pub fn open(parent: *mut c_void, state: Rc<Publication>) -> Result<Self, String> {
        if !is_main() {
            return Err("Cocoa export panel requires the main thread".into());
        }
        if parent.is_null() {
            return Err("Missing host NSView parent".into());
        }
        let _pool = Pool::new();
        unsafe {
            let initialize: unsafe extern "C" fn(Obj, Obj, Rect) -> Obj =
                std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            let view = initialize(
                send(objc_getClass(c"NSView".as_ptr()), c"alloc"),
                selector(c"initWithFrame:"),
                Rect {
                    origin: Point::default(),
                    size: Size {
                        width: 560.0,
                        height: 180.0,
                    },
                },
            );
            if view.is_null() {
                return Err("Cocoa panel allocation failed".into());
            }
            let mut context = Box::new(Context {
                state,
                path: ptr::null_mut(),
                status: ptr::null_mut(),
            });
            let name = CString::new(format!("WindfallPanelAction_{:p}", &*context)).unwrap();
            let class =
                objc_allocateClassPair(objc_getClass(c"NSObject".as_ptr()), name.as_ptr(), 0);
            if class.is_null() {
                void(view, c"release");
                return Err("Cocoa panel action class failed".into());
            }
            let implementation: unsafe extern "C" fn() =
                std::mem::transmute(load as unsafe extern "C" fn(Obj, Obj, Obj));
            if class_addIvar(
                class,
                c"windfallContext".as_ptr(),
                size_of::<*mut Context>(),
                align_of::<*mut Context>().trailing_zeros() as u8,
                c"^v".as_ptr(),
            ) == 0
                || class_addMethod(
                    class,
                    selector(c"loadProgram:"),
                    implementation,
                    c"v@:@".as_ptr(),
                ) == 0
            {
                objc_disposeClassPair(class);
                void(view, c"release");
                return Err("Cocoa panel action registration failed".into());
            }
            objc_registerClassPair(class);
            let target = send(send(class, c"alloc"), c"init");
            if target.is_null() {
                objc_disposeClassPair(class);
                void(view, c"release");
                return Err("Cocoa panel target failed".into());
            }
            object_setInstanceVariable(
                target,
                c"windfallContext".as_ptr(),
                (&mut *context as *mut Context).cast(),
            );
            control(
                c"NSTextField",
                view,
                "Windfall — 16 stereo outputs",
                12.0,
                140.0,
                530.0,
                24.0,
                false,
            );
            control(
                c"NSTextField",
                view,
                "Exported program state file (windfall-state.json):",
                12.0,
                112.0,
                530.0,
                22.0,
                false,
            );
            context.path = control(c"NSTextField", view, "", 12.0, 78.0, 410.0, 28.0, true);
            let button = control(
                c"NSButton",
                view,
                "Load program",
                430.0,
                78.0,
                118.0,
                28.0,
                false,
            );
            context.status = control(
                c"NSTextField",
                view,
                "Host transport, tempo and meter take precedence over the document.",
                12.0,
                12.0,
                530.0,
                58.0,
                false,
            );
            let panel = Self {
                view,
                target,
                class,
                context,
            };
            if panel.context.path.is_null() || panel.context.status.is_null() || button.is_null() {
                return Err("Cocoa panel controls failed".into());
            }
            object(button, c"setTarget:", target);
            object(button, c"setAction:", selector(c"loadProgram:"));
            object(parent, c"addSubview:", view);
            Ok(panel)
        }
    }
    pub fn visible(&self, visible: bool) {
        unsafe {
            boolean(self.view, c"setHidden:", !visible);
        }
    }
}
impl Drop for Panel {
    fn drop(&mut self) {
        debug_assert!(is_main());
        unsafe {
            void(self.view, c"removeFromSuperview");
            void(self.view, c"release"); // controls/actions gone before context
            object_setInstanceVariable(self.target, c"windfallContext".as_ptr(), ptr::null_mut());
            void(self.target, c"release");
            objc_disposeClassPair(self.class);
        }
    }
}
