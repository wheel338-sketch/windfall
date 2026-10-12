//! A real, fixed-size Win32 editor for the healthy VST3 gain fixture.
#![allow(non_snake_case)]

use std::{
    cell::Cell,
    ffi::{CStr, c_void},
    ptr,
};
use vst3::{Class, ComPtr, ComWrapper, Steinberg::*};
use windows_sys::Win32::{
    Foundation::HWND,
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, MoveWindow, WS_CHILD, WS_VISIBLE},
};
use windows_sys::core::w;

const WIDTH: i32 = 320;
const HEIGHT: i32 = 200;

pub(super) fn create() -> *mut IPlugView {
    let view: ComPtr<IPlugView> = ComWrapper::new(View {
        window: Cell::new(ptr::null_mut()),
    })
    .to_com_ptr()
    .expect("fixture view interface");
    view.into_raw()
}

struct View {
    window: Cell<HWND>,
}
impl Class for View {
    type Interfaces = (IPlugView,);
}
impl View {
    fn close(&self) {
        let window = self.window.replace(ptr::null_mut());
        if !window.is_null() {
            // SAFETY: only our child window, on the editor's owning main thread.
            unsafe {
                DestroyWindow(window);
            }
        }
    }
}
impl Drop for View {
    fn drop(&mut self) {
        self.close();
    }
}

impl IPlugViewTrait for View {
    unsafe fn isPlatformTypeSupported(&self, platform: FIDString) -> tresult {
        // SAFETY: the SDK caller supplies a live null-terminated platform string.
        if !platform.is_null() && unsafe { CStr::from_ptr(platform) } == c"HWND" {
            kResultOk
        } else {
            kResultFalse
        }
    }
    unsafe fn attached(&self, parent: *mut c_void, platform: FIDString) -> tresult {
        if parent.is_null()
            || !self.window.get().is_null()
            || unsafe { self.isPlatformTypeSupported(platform) } != kResultOk
        {
            return kInvalidArgument;
        }
        // SAFETY: the host supplies its live parent HWND on the main thread.
        // STATIC is a system window class; no DLL-owned window procedure can
        // outlive fixture unload. The returned child belongs to this view.
        let window = unsafe {
            CreateWindowExW(
                0,
                w!("STATIC"),
                w!("Windfall VST3 fixture editor"),
                WS_CHILD | WS_VISIBLE,
                0,
                0,
                WIDTH,
                HEIGHT,
                parent,
                ptr::null_mut(),
                GetModuleHandleW(ptr::null()),
                ptr::null(),
            )
        };
        if window.is_null() {
            kResultFalse
        } else {
            self.window.set(window);
            kResultOk
        }
    }
    unsafe fn removed(&self) -> tresult {
        self.close();
        kResultOk
    }
    unsafe fn onWheel(&self, _distance: f32) -> tresult {
        kResultFalse
    }
    unsafe fn onKeyDown(&self, _key: char16, _code: int16, _modifiers: int16) -> tresult {
        kResultFalse
    }
    unsafe fn onKeyUp(&self, _key: char16, _code: int16, _modifiers: int16) -> tresult {
        kResultFalse
    }
    unsafe fn getSize(&self, rect: *mut ViewRect) -> tresult {
        if rect.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: SDK writable rectangle, checked non-null.
        unsafe {
            rect.write(ViewRect {
                left: 0,
                top: 0,
                right: WIDTH,
                bottom: HEIGHT,
            });
        }
        kResultOk
    }
    unsafe fn onSize(&self, rect: *mut ViewRect) -> tresult {
        if rect.is_null() {
            return kInvalidArgument;
        }
        // SAFETY: SDK readable rectangle, checked non-null.
        let rect = unsafe { &*rect };
        if rect.right - rect.left != WIDTH || rect.bottom - rect.top != HEIGHT {
            return kResultFalse;
        }
        if !self.window.get().is_null() {
            // SAFETY: view-owned live child window, fixed checked dimensions.
            unsafe {
                MoveWindow(self.window.get(), 0, 0, WIDTH, HEIGHT, 1);
            }
        }
        kResultOk
    }
    unsafe fn onFocus(&self, _state: TBool) -> tresult {
        kResultOk
    }
    unsafe fn setFrame(&self, _frame: *mut IPlugFrame) -> tresult {
        // Fixed-size editor never calls back through a borrowed host frame.
        kResultOk
    }
    unsafe fn canResize(&self) -> tresult {
        kResultFalse
    }
    unsafe fn checkSizeConstraint(&self, rect: *mut ViewRect) -> tresult {
        // SAFETY: identical writable rectangle contract to getSize.
        unsafe { self.getSize(rect) }
    }
}
