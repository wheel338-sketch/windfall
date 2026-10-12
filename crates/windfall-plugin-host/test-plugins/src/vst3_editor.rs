//! Independent Win32 SDK view. The button edits the same atomic parameter as
//! processing/state, and emits the real begin/perform/end host gesture.
#![allow(non_snake_case)]
use std::{cell::Cell, ffi::c_void, ptr, sync::{Arc, atomic::{AtomicU64, Ordering}}};
use vst3::{Class, ComPtr, ComWrapper, Steinberg::{*, Vst::*}};
use windows_sys::{core::w, Win32::{Foundation::*, System::LibraryLoader::{GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT}, UI::WindowsAndMessaging::*}};
fn module() -> HMODULE {
    let mut module = ptr::null_mut();
    // SAFETY: callback address belongs to this live fixture module; borrow its
    // handle without retaining it and masking actual loader/unload behavior.
    unsafe { GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, procedure as *const () as *const u16, &mut module); }
    module
}

struct Control {
    gain: Arc<AtomicU64>,
    handler: Option<ComPtr<IComponentHandler>>,
}
impl Control {
    fn edit(&self) {
        let before = f64::from_bits(self.gain.load(Ordering::Relaxed));
        let value: f64 = if before < 0.5 { 0.75 } else { 0.25 };
        self.gain.store(value.to_bits(), Ordering::Relaxed);
        if let Some(handler) = &self.handler {
            // SAFETY: editor gesture on the native creating/UI thread.
            unsafe {
                handler.beginEdit(7);
                handler.performEdit(7, value);
                handler.endEdit(7);
                if let Some(dirty) = handler.cast::<IComponentHandler2>() { dirty.setDirty(1); }
            }
        }
    }
}
unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // SAFETY: Win32 supplies valid creation storage; Control outlives this HWND.
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lp as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        if message == WM_COMMAND && wp & 0xffff == 7 && (wp >> 16) & 0xffff == BN_CLICKED as usize {
            let control = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Control;
            if let Some(control) = control.as_ref() {
                control.edit();
                let text: Vec<u16> = format!("Gain {:.2} (click to change)", f64::from_bits(control.gain.load(Ordering::Relaxed)) * 2.0).encode_utf16().chain(Some(0)).collect();
                SetWindowTextW(GetDlgItem(hwnd, 7), text.as_ptr());
            }
            return 0;
        }
        if message == WM_NCDESTROY { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0); }
        DefWindowProcW(hwnd, message, wp, lp)
    }
}
struct View {
    control: Box<Control>,
    window: Cell<HWND>,
    size: Cell<(i32, i32)>,
    frame: Cell<*mut IPlugFrame>,
}
impl Class for View { type Interfaces = (IPlugView, IPlugViewContentScaleSupport); }
pub(super) fn create(gain: Arc<AtomicU64>, handler: Option<ComPtr<IComponentHandler>>) -> *mut IPlugView {
    ComWrapper::new(View { control: Box::new(Control { gain, handler }), window: Cell::new(ptr::null_mut()), size: Cell::new((320, 200)), frame: Cell::new(ptr::null_mut()) }).to_com_ptr::<IPlugView>().unwrap().into_raw()
}
impl IPlugViewTrait for View {
    unsafe fn isPlatformTypeSupported(&self, kind: FIDString) -> tresult {
        if !kind.is_null() && unsafe { std::ffi::CStr::from_ptr(kind) } == c"HWND" { kResultOk } else { kResultFalse }
    }
    unsafe fn attached(&self, parent: *mut c_void, kind: FIDString) -> tresult {
        if parent.is_null() || !self.window.get().is_null() || unsafe { self.isPlatformTypeSupported(kind) } != kResultOk { return kInvalidArgument; }
        {
            // SAFETY: static class name/callback retained by this fixture module.
            unsafe {
                let class = WNDCLASSW { lpfnWndProc: Some(procedure), hInstance: module(), lpszClassName: w!("WindfallIndependentVst3Editor"), ..std::mem::zeroed() };
                RegisterClassW(&class);
            }
        }
        let (width, height) = self.size.get();
        // SAFETY: owner-thread child windows, parent provided by SDK host.
        let window = unsafe { CreateWindowExW(0, w!("WindfallIndependentVst3Editor"), w!("Windfall VST3 fixture"), WS_CHILD | WS_VISIBLE, 0, 0, width, height, parent, ptr::null_mut(), module(), (&*self.control as *const Control).cast()) };
        if window.is_null() { return kResultFalse; }
        self.window.set(window);
        let text: Vec<u16> = format!("Gain {:.2} (click to change)", f64::from_bits(self.control.gain.load(Ordering::Relaxed)) * 2.0).encode_utf16().chain(Some(0)).collect();
        let button = unsafe { CreateWindowExW(0, w!("BUTTON"), text.as_ptr(), WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON as u32, 8, 8, width - 16, height - 16, window, 7usize as HMENU, module(), ptr::null()) };
        if button.is_null() { unsafe { self.removed(); } return kResultFalse; }
        kResultOk
    }
    unsafe fn removed(&self) -> tresult {
        let window = self.window.replace(ptr::null_mut());
        if !window.is_null() {
            unsafe {
                DestroyWindow(window);
                // Succeeds only when the last live view of this module closes.
                // Never leave a class callback pointing into an unloaded DLL.
                UnregisterClassW(w!("WindfallIndependentVst3Editor"), module());
            }
        }
        kResultOk
    }
    unsafe fn onWheel(&self, _: f32) -> tresult { kResultFalse }
    unsafe fn onKeyDown(&self, _: char16, _: int16, _: int16) -> tresult { kResultFalse }
    unsafe fn onKeyUp(&self, _: char16, _: int16, _: int16) -> tresult { kResultFalse }
    unsafe fn getSize(&self, rect: *mut ViewRect) -> tresult {
        if rect.is_null() { return kInvalidArgument; }
        let (width, height) = self.size.get();
        unsafe { rect.write(ViewRect { left: 0, top: 0, right: width, bottom: height }); }
        kResultOk
    }
    unsafe fn onSize(&self, rect: *mut ViewRect) -> tresult {
        if rect.is_null() { return kInvalidArgument; }
        let rect = unsafe { &*rect };
        let Some(width) = rect.right.checked_sub(rect.left) else { return kInvalidArgument; };
        let Some(height) = rect.bottom.checked_sub(rect.top) else { return kInvalidArgument; };
        if !(64..=4096).contains(&width) || !(32..=4096).contains(&height) { return kInvalidArgument; }
        self.size.set((width, height));
        if !self.window.get().is_null() {
            unsafe {
                MoveWindow(self.window.get(), 0, 0, width, height, 1);
                MoveWindow(GetDlgItem(self.window.get(), 7), 8, 8, width - 16, height - 16, 1);
            }
        }
        kResultOk
    }
    unsafe fn onFocus(&self, _: TBool) -> tresult { kResultOk }
    unsafe fn setFrame(&self, frame: *mut IPlugFrame) -> tresult { self.frame.set(frame); kResultOk }
    unsafe fn canResize(&self) -> tresult { kResultOk }
    unsafe fn checkSizeConstraint(&self, rect: *mut ViewRect) -> tresult {
        if rect.is_null() { return kInvalidArgument; }
        let rect = unsafe { &mut *rect };
        rect.right = rect.left.saturating_add(rect.right.saturating_sub(rect.left).clamp(64, 4096));
        rect.bottom = rect.top.saturating_add(rect.bottom.saturating_sub(rect.top).clamp(32, 4096));
        kResultOk
    }
}
impl IPlugViewContentScaleSupportTrait for View {
    unsafe fn setContentScaleFactor(&self, factor: f32) -> tresult {
        if !factor.is_finite() || !(0.25..=8.0).contains(&factor) { return kInvalidArgument; }
        let mut rect = ViewRect { left: 0, top: 0, right: (320.0 * factor).round() as i32, bottom: (200.0 * factor).round() as i32 };
        unsafe { self.onSize(&mut rect) }
    }
}
impl Drop for View { fn drop(&mut self) { unsafe { self.removed(); } } }
