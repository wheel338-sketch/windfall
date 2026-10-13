//! AUv2 Cocoa factory embedding. All calls and retained objects belong to the
//! Cocoa main thread. Handwritten typed Objective-C selectors, no vendor SDK.
use super::sys::*;
use crate::{EditorError, EditorInfo, EditorOptions};
use std::{
    ffi::{CString, c_char, c_void},
    ptr,
};
type Obj = *mut c_void;
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Point {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Size {
    width: f64,
    height: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect {
    origin: Point,
    size: Size,
}
#[repr(C)]
struct CocoaInfo {
    bundle_url: Cf,
    classes: [Cf; 1],
}
struct Metadata {
    storage: Vec<usize>,
    bytes: u32,
}
impl Drop for Metadata {
    fn drop(&mut self) {
        // kAudioUnitProperty_CocoaUI returns owned CFURL and CFString refs.
        let count = self.bytes as usize / size_of::<usize>();
        for value in &self.storage[..count.min(self.storage.len())] {
            if *value != 0 {
                unsafe {
                    CFRelease(*value as Cf);
                }
            }
        }
    }
}
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}
#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Obj;
    fn sel_registerName(name: *const c_char) -> Obj;
    fn objc_msgSend();
    #[cfg(target_arch = "x86_64")]
    fn objc_msgSend_stret();
}
unsafe fn selector(name: &'static [u8]) -> Obj {
    unsafe { sel_registerName(name.as_ptr().cast()) }
}
unsafe fn send(receiver: Obj, name: &'static [u8]) -> Obj {
    let call: unsafe extern "C" fn(Obj, Obj) -> Obj =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe { call(receiver, selector(name)) }
}
unsafe fn void_send(receiver: Obj, name: &'static [u8]) {
    let call: unsafe extern "C" fn(Obj, Obj) =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe {
        call(receiver, selector(name));
    }
}
unsafe fn object(receiver: Obj, name: &'static [u8], argument: Obj) {
    let call: unsafe extern "C" fn(Obj, Obj, Obj) =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe {
        call(receiver, selector(name), argument);
    }
}
unsafe fn bool_arg(receiver: Obj, name: &'static [u8], value: bool) {
    let call: unsafe extern "C" fn(Obj, Obj, i8) =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe {
        call(receiver, selector(name), i8::from(value));
    }
}
unsafe fn frame(view: Obj) -> Rect {
    #[cfg(target_arch = "aarch64")]
    {
        let call: unsafe extern "C" fn(Obj, Obj) -> Rect =
            unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
        unsafe { call(view, selector(b"frame\0")) }
    }
    #[cfg(target_arch = "x86_64")]
    {
        let call: unsafe extern "C" fn(*mut Rect, Obj, Obj) =
            unsafe { std::mem::transmute(objc_msgSend_stret as unsafe extern "C" fn()) };
        let mut rect = Rect::default();
        unsafe {
            call(&mut rect, view, selector(b"frame\0"));
        }
        rect
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        let _ = view;
        Rect::default()
    }
}
pub fn available(unit: Unit) -> bool {
    let (mut size, mut writable) = (0, 0);
    unsafe {
        AudioUnitGetPropertyInfo(unit, 31, 0, 0, &mut size, &mut writable) == 0
            && size as usize >= size_of::<CocoaInfo>()
    }
}
fn main_thread() -> bool {
    let call: unsafe extern "C" fn(Obj, Obj) -> i8 =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe {
        call(
            objc_getClass(c"NSThread".as_ptr()),
            selector(b"isMainThread\0"),
        ) != 0
    }
}
pub struct Editor {
    bundle: Cf,
    factory: Obj,
    view: Obj,
    window: Obj,
    info: EditorInfo,
}
impl Editor {
    pub fn open(unit: Unit, options: &EditorOptions) -> Result<Self, EditorError> {
        if !main_thread() {
            return Err(EditorError::Failed(
                "AU Cocoa editor requires the main thread".into(),
            ));
        }
        if options.scale != 1.0 {
            return Err(EditorError::UnsupportedScale);
        }
        let (mut size, mut writable) = (0, 0);
        if unsafe { AudioUnitGetPropertyInfo(unit, 31, 0, 0, &mut size, &mut writable) } != 0
            || (size as usize) < size_of::<CocoaInfo>()
            || size > 64 * 1024
        {
            return Err(EditorError::NoEditor);
        }
        // pointer-aligned backing storage for variable-length CFString class list.
        let mut storage = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
        if unsafe { AudioUnitGetProperty(unit, 31, 0, 0, storage.as_mut_ptr().cast(), &mut size) }
            != 0
        {
            return Err(EditorError::Failed(
                "AU Cocoa factory metadata failed".into(),
            ));
        }
        let metadata = Metadata {
            storage,
            bytes: size,
        };
        if size as usize > metadata.storage.len() * size_of::<usize>()
            || (size as usize) < size_of::<CocoaInfo>()
            || !(size as usize).is_multiple_of(size_of::<usize>())
        {
            return Err(EditorError::Failed("invalid AU Cocoa metadata size".into()));
        }
        let info = unsafe { &*metadata.storage.as_ptr().cast::<CocoaInfo>() };
        let bundle = unsafe { CFBundleCreate(ptr::null(), info.bundle_url) };
        if bundle.is_null() {
            return Err(EditorError::NoEditor);
        }
        if unsafe { CFBundleLoadExecutable(bundle) } == 0 {
            unsafe {
                CFRelease(bundle);
            }
            return Err(EditorError::Failed("AU Cocoa bundle load failed".into()));
        }
        let count = size as usize / size_of::<usize>() - 1;
        let names =
            unsafe { std::slice::from_raw_parts(ptr::addr_of!(info.classes).cast::<Cf>(), count) };
        let mut factory = ptr::null_mut();
        for name in names {
            let Ok(name) = CString::new(super::text(*name)) else {
                continue;
            };
            let class = unsafe { objc_getClass(name.as_ptr()) };
            if class.is_null() {
                continue;
            }
            factory = unsafe { send(send(class, b"alloc\0"), b"init\0") };
            let responds: unsafe extern "C" fn(Obj, Obj, Obj) -> i8 =
                unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
            if !factory.is_null()
                && unsafe {
                    responds(
                        factory,
                        selector(b"respondsToSelector:\0"),
                        selector(b"uiViewForAudioUnit:withSize:\0"),
                    )
                } != 0
            {
                break;
            }
            if !factory.is_null() {
                unsafe {
                    void_send(factory, b"release\0");
                }
                factory = ptr::null_mut();
            }
        }
        if factory.is_null() {
            unsafe {
                CFRelease(bundle);
            }
            return Err(EditorError::NoEditor);
        }
        let make_view: unsafe extern "C" fn(Obj, Obj, Unit, Size) -> Obj =
            unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
        let view = unsafe {
            make_view(
                factory,
                selector(b"uiViewForAudioUnit:withSize:\0"),
                unit,
                Size {
                    width: 640.0,
                    height: 480.0,
                },
            )
        };
        if view.is_null() {
            unsafe {
                void_send(factory, b"release\0");
                CFRelease(bundle);
            }
            return Err(EditorError::NoEditor);
        }
        unsafe {
            send(view, b"retain\0");
        }
        let rect = unsafe { frame(view) };
        if !rect.size.width.is_finite()
            || !rect.size.height.is_finite()
            || !(1.0..=8192.0).contains(&rect.size.width)
            || !(1.0..=8192.0).contains(&rect.size.height)
        {
            unsafe {
                void_send(view, b"release\0");
                void_send(factory, b"release\0");
                CFRelease(bundle);
            }
            return Err(EditorError::Failed(
                "AU Cocoa view size exceeds bound".into(),
            ));
        }
        let allocate = unsafe { send(objc_getClass(c"NSWindow".as_ptr()), b"alloc\0") };
        let initialize: unsafe extern "C" fn(Obj, Obj, Rect, usize, usize, i8) -> Obj =
            unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
        let window = unsafe {
            initialize(
                allocate,
                selector(b"initWithContentRect:styleMask:backing:defer:\0"),
                Rect {
                    origin: Point {
                        x: options.position.map_or(100.0, |p| f64::from(p.0)),
                        y: options.position.map_or(100.0, |p| f64::from(p.1)),
                    },
                    size: rect.size,
                },
                1 | 2 | 4 | 8,
                2,
                0,
            )
        };
        if window.is_null() {
            unsafe {
                void_send(view, b"release\0");
                void_send(factory, b"release\0");
                CFRelease(bundle);
            }
            return Err(EditorError::Failed("AU editor window failed".into()));
        }
        unsafe {
            bool_arg(window, b"setReleasedWhenClosed:\0", false);
            object(window, b"setContentView:\0", view);
            if options.take_focus {
                object(window, b"makeKeyAndOrderFront:\0", ptr::null_mut());
            } else {
                object(window, b"orderFront:\0", ptr::null_mut());
            }
            if !options.detached {
                if let Some(owner) = options.owner_window {
                    let attach: unsafe extern "C" fn(Obj, Obj, Obj, isize) =
                        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
                    attach(
                        owner as Obj,
                        selector(b"addChildWindow:ordered:\0"),
                        window,
                        1,
                    );
                }
            }
        }
        Ok(Self {
            bundle,
            factory,
            view,
            window,
            info: EditorInfo {
                window: Some(window as usize),
                width: rect.size.width as u32,
                height: rect.size.height as u32,
                resizable: true,
            },
        })
    }
    pub fn info(&self) -> EditorInfo {
        self.info
    }
    pub fn visible(&self) -> bool {
        let call: unsafe extern "C" fn(Obj, Obj) -> i8 =
            unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
        unsafe { call(self.window, selector(b"isVisible\0")) != 0 }
    }
}
impl Drop for Editor {
    fn drop(&mut self) {
        unsafe {
            let parent = send(self.window, b"parentWindow\0");
            if !parent.is_null() {
                object(parent, b"removeChildWindow:\0", self.window);
            }
            void_send(self.view, b"removeFromSuperview\0");
            void_send(self.window, b"close\0");
            void_send(self.window, b"release\0");
            void_send(self.view, b"release\0");
            void_send(self.factory, b"release\0");
            CFRelease(self.bundle);
        }
    }
}
