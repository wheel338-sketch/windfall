//! Small typed AppKit/Objective-C ABI shared by host windows and exported panels.
//! BOOL is signed char on Intel and bool on arm64; both have one-byte ABI.
use std::ffi::{c_char, c_void};
pub type Obj = *mut c_void;
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub fn CFRunLoopRunInMode(mode: *const c_void, seconds: f64, return_after_source: u8) -> i32;
}
#[link(name = "objc")]
unsafe extern "C" {
    pub fn objc_getClass(name: *const c_char) -> Obj;
    pub fn sel_registerName(name: *const c_char) -> Obj;
    pub fn objc_msgSend();
    #[cfg(target_arch = "x86_64")]
    pub fn objc_msgSend_stret();
    pub fn objc_allocateClassPair(superclass: Obj, name: *const c_char, extra_bytes: usize) -> Obj;
    pub fn objc_registerClassPair(class: Obj);
    pub fn objc_disposeClassPair(class: Obj);
    pub fn class_addMethod(
        class: Obj,
        selector: Obj,
        implementation: unsafe extern "C" fn(),
        types: *const c_char,
    ) -> i8;
    pub fn class_addIvar(
        class: Obj,
        name: *const c_char,
        size: usize,
        alignment: u8,
        types: *const c_char,
    ) -> i8;
    pub fn object_getInstanceVariable(
        object: Obj,
        name: *const c_char,
        value: *mut *mut c_void,
    ) -> Obj;
    pub fn object_setInstanceVariable(object: Obj, name: *const c_char, value: *mut c_void) -> Obj;
    pub fn pthread_main_np() -> i32;
}
pub fn is_main() -> bool {
    unsafe { pthread_main_np() != 0 }
}
pub unsafe fn selector(name: &std::ffi::CStr) -> Obj {
    unsafe { sel_registerName(name.as_ptr()) }
}
pub unsafe fn send(receiver: Obj, name: &std::ffi::CStr) -> Obj {
    let call: unsafe extern "C" fn(Obj, Obj) -> Obj =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe { call(receiver, selector(name)) }
}
pub unsafe fn void(receiver: Obj, name: &std::ffi::CStr) {
    let call: unsafe extern "C" fn(Obj, Obj) =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe {
        call(receiver, selector(name));
    }
}
pub unsafe fn object(receiver: Obj, name: &std::ffi::CStr, value: Obj) {
    let call: unsafe extern "C" fn(Obj, Obj, Obj) =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe {
        call(receiver, selector(name), value);
    }
}
pub unsafe fn boolean(receiver: Obj, name: &std::ffi::CStr, value: bool) {
    let call: unsafe extern "C" fn(Obj, Obj, i8) =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe {
        call(receiver, selector(name), i8::from(value));
    }
}
pub unsafe fn bool_result(receiver: Obj, name: &std::ffi::CStr) -> bool {
    let call: unsafe extern "C" fn(Obj, Obj) -> i8 =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe { call(receiver, selector(name)) != 0 }
}
pub unsafe fn double_result(receiver: Obj, name: &std::ffi::CStr) -> f64 {
    let call: unsafe extern "C" fn(Obj, Obj) -> f64 =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe { call(receiver, selector(name)) }
}
pub unsafe fn rect(receiver: Obj, name: &std::ffi::CStr) -> Rect {
    #[cfg(target_arch = "aarch64")]
    {
        let call: unsafe extern "C" fn(Obj, Obj) -> Rect =
            unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
        unsafe { call(receiver, selector(name)) }
    }
    #[cfg(target_arch = "x86_64")]
    {
        let call: unsafe extern "C" fn(*mut Rect, Obj, Obj) =
            unsafe { std::mem::transmute(objc_msgSend_stret as unsafe extern "C" fn()) };
        let mut value = Rect::default();
        unsafe {
            call(&mut value, receiver, selector(name));
        }
        value
    }
}
pub unsafe fn set_rect(receiver: Obj, name: &std::ffi::CStr, value: Rect) {
    let call: unsafe extern "C" fn(Obj, Obj, Rect) =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe {
        call(receiver, selector(name), value);
    }
}
pub unsafe fn string(text: &str) -> Obj {
    let call: unsafe extern "C" fn(Obj, Obj, *const c_void, usize, usize) -> Obj =
        unsafe { std::mem::transmute(objc_msgSend as unsafe extern "C" fn()) };
    unsafe {
        call(
            send(objc_getClass(c"NSString".as_ptr()), c"alloc"),
            selector(c"initWithBytes:length:encoding:"),
            text.as_ptr().cast(),
            text.len(),
            4,
        )
    }
}
pub struct Pool(Obj);
impl Pool {
    pub fn new() -> Self {
        Self(unsafe {
            send(
                send(objc_getClass(c"NSAutoreleasePool".as_ptr()), c"alloc"),
                c"init",
            )
        })
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        unsafe {
            void(self.0, c"drain");
        }
    }
}
