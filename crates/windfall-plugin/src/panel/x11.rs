//! XEmbed panel. CLAP's host timer or VST3's frame IRunLoop services this
//! connection on the host GUI thread; no polling thread calls Publication.
#[path = "../../../windfall-plugin-host/src/gui/x11_ffi.rs"]
#[allow(dead_code)]
mod ffi;
use super::*;
use ffi::*;
use std::{cell::RefCell, ptr, rc::Weak};
use vst3::{
    Class, ComPtr, ComRef, ComWrapper,
    Steinberg::{Linux::*, *},
};
thread_local! { static OWNER: u8 = const { 0 }; }
fn token() -> usize {
    OWNER.with(|value| std::ptr::from_ref(value) as usize)
}
struct Inner {
    display: *mut Display,
    window: Xid,
    gc: *mut c_void,
    owner: usize,
    input_method: *mut c_void,
    input_context: *mut c_void,
    state: Rc<Publication>,
    words: Words,
    path: String,
    status: String,
}
impl Inner {
    fn draw(&self) {
        unsafe {
            XClearWindow(self.display, self.window);
            XSetForeground(self.display, self.gc, 0xffffff);
            let text = |label: &str, x, y| {
                // Core X fonts are Latin-1, which has every letter of the
                // panel's languages; a dash stands for a long dash.
                let bytes: Vec<u8> = label
                    .chars()
                    .map(|c| match c {
                        '—' | '–' => b'-',
                        c if u32::from(c) < 256 => c as u8,
                        _ => b'?',
                    })
                    .collect();
                XDrawString(
                    self.display,
                    self.window,
                    self.gc,
                    x,
                    y,
                    bytes.as_ptr().cast(),
                    bytes.len().min(4096) as i32,
                );
            };
            text(&self.words.title(), 12, 25);
            text(&self.words.state_file(), 12, 52);
            XDrawRectangle(self.display, self.window, self.gc, 12, 64, 408, 28);
            text(&self.path, 18, 83);
            XDrawRectangle(self.display, self.window, self.gc, 430, 64, 118, 28);
            text(&self.words.load(), 438, 83);
            text(&self.status, 12, 122);
            XFlush(self.display);
        }
    }
    fn load(&mut self) {
        self.status = self.words.loaded(&self.state, &self.path);
    }
    fn poll(&mut self) {
        if token() != self.owner {
            return;
        }
        unsafe {
            for _ in 0..256 {
                if XPending(self.display) == 0 {
                    break;
                }
                let mut event = Event { pad: [0; 24] };
                XNextEvent(self.display, &mut event);
                if XFilterEvent(&mut event, self.window) != 0 {
                    continue;
                }
                if event.any.window != self.window {
                    continue;
                }
                match event.kind {
                    9 => {
                        if !self.input_context.is_null() {
                            XSetICFocus(self.input_context);
                        }
                    }
                    10 => {
                        if !self.input_context.is_null() {
                            XUnsetICFocus(self.input_context);
                        }
                    }
                    12 => self.draw(), // Expose
                    4 => {
                        // XButtonEvent has the XKeyEvent prefix through state;
                        // use only common x/y fields, not keycode.
                        let click = event.key;
                        XSetInputFocus(self.display, self.window, 2, 0);
                        if !self.input_context.is_null() {
                            XSetICFocus(self.input_context);
                        }
                        if (430..=548).contains(&click.x) && (64..=92).contains(&click.y) {
                            self.load();
                        }
                        self.draw();
                    }
                    2 => {
                        let mut key = event.key;
                        let mut bytes = [0 as c_char; 256];
                        let mut symbol = 0;
                        let mut status = 0;
                        let length = if self.input_context.is_null() {
                            XLookupString(
                                &mut key,
                                bytes.as_mut_ptr(),
                                bytes.len() as i32,
                                &mut symbol,
                                ptr::null_mut(),
                            )
                        } else {
                            Xutf8LookupString(
                                self.input_context,
                                &mut key,
                                bytes.as_mut_ptr(),
                                bytes.len() as i32,
                                &mut symbol,
                                &mut status,
                            )
                        };
                        match symbol {
                            0xff08 | 0xffff => {
                                self.path.pop();
                            }
                            0xff0d => self.load(),
                            _ if length > 0
                                && length as usize <= bytes.len()
                                && status != -1
                                && self.path.len() < 4096 =>
                            {
                                let text = std::slice::from_raw_parts(
                                    bytes.as_ptr().cast::<u8>(),
                                    length as usize,
                                );
                                let text = if self.input_context.is_null() {
                                    text.iter()
                                        .map(|byte| char::from(*byte))
                                        .collect::<String>()
                                } else {
                                    String::from_utf8_lossy(text).into_owned()
                                };
                                for character in text.chars().filter(|c| !c.is_control()) {
                                    if self.path.len() + character.len_utf8() <= 4096 {
                                        self.path.push(character);
                                    }
                                }
                            }
                            _ => {}
                        }
                        self.draw();
                    }
                    _ => {}
                }
            }
        }
    }
}
impl Drop for Inner {
    fn drop(&mut self) {
        debug_assert_eq!(token(), self.owner);
        unsafe {
            if !self.input_context.is_null() {
                XDestroyIC(self.input_context);
            }
            if !self.input_method.is_null() {
                XCloseIM(self.input_method);
            }
            XFreeGC(self.display, self.gc);
            XDestroyWindow(self.display, self.window);
            XCloseDisplay(self.display);
        }
    }
}
struct Tick {
    inner: Weak<RefCell<Inner>>,
}
impl Class for Tick {
    type Interfaces = (ITimerHandler,);
}
#[allow(non_snake_case)]
impl ITimerHandlerTrait for Tick {
    unsafe fn onTimer(&self) {
        if let Some(inner) = self.inner.upgrade()
            && let Ok(mut inner) = inner.try_borrow_mut()
        {
            inner.poll();
        }
    }
}
struct Timer {
    run_loop: ComPtr<IRunLoop>,
    tick: ComPtr<ITimerHandler>,
}
impl Drop for Timer {
    fn drop(&mut self) {
        unsafe {
            self.run_loop.unregisterTimer(self.tick.as_ptr());
        }
    }
}
pub struct Panel {
    timer: Option<Timer>,
    inner: Rc<RefCell<Inner>>,
}
impl Panel {
    pub fn open(parent: *mut c_void, state: Rc<Publication>) -> Result<Self, String> {
        let words = Words::chosen();
        if parent.is_null() {
            return Err("Missing host X11 window id".into());
        }
        unsafe {
            let display = XOpenDisplay(ptr::null());
            if display.is_null() {
                return Err("X11 display unavailable".into());
            }
            let window = XCreateSimpleWindow(
                display,
                parent as usize as Xid,
                0,
                0,
                560,
                180,
                0,
                0,
                0x202020,
            );
            if window == 0 {
                XCloseDisplay(display);
                return Err("X11 panel creation failed".into());
            }
            XSelectInput(display, window, (1 << 15) | (1 << 0) | (1 << 2) | (1 << 21)); // Exposure, key, button, focus
            // XEmbed version 0, XEMBED_MAPPED. Property format is 32 even on
            // LP64; Xlib represents its input elements as unsigned long.
            let atom = XInternAtom(display, c"_XEMBED_INFO".as_ptr(), 0);
            let info: [Xid; 2] = [0, 1];
            XChangeProperty(display, window, atom, atom, 32, 0, info.as_ptr().cast(), 2);
            let gc = XCreateGC(display, window, 0, ptr::null_mut());
            if gc.is_null() {
                XDestroyWindow(display, window);
                XCloseDisplay(display);
                return Err("X11 panel drawing context failed".into());
            }
            XMapWindow(display, window);
            XFlush(display);
            let input_method = XOpenIM(display, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            let input_context = if input_method.is_null() {
                ptr::null_mut()
            } else {
                XCreateIC(
                    input_method,
                    c"inputStyle".as_ptr(),
                    0x408 as Xid,
                    c"clientWindow".as_ptr(),
                    window,
                    c"focusWindow".as_ptr(),
                    window,
                    ptr::null::<c_char>(),
                )
            };
            let inner = Rc::new(RefCell::new(Inner {
                display,
                window,
                gc,
                owner: token(),
                input_method,
                input_context,
                state: state.clone(),
                words,
                path: String::new(),
                status: super::status_text(&state, words.host_first(), words),
            }));
            inner.borrow().draw();
            Ok(Self { timer: None, inner })
        }
    }
    pub fn visible(&self, visible: bool) {
        let inner = self.inner.borrow();
        unsafe {
            let atom = XInternAtom(inner.display, c"_XEMBED_INFO".as_ptr(), 0);
            let info: [Xid; 2] = [0, if visible { 1 } else { 0 }];
            XChangeProperty(
                inner.display,
                inner.window,
                atom,
                atom,
                32,
                0,
                info.as_ptr().cast(),
                2,
            );
            if visible {
                XMapWindow(inner.display, inner.window);
            } else {
                XUnmapWindow(inner.display, inner.window);
            }
            XFlush(inner.display);
        }
    }
    pub fn poll(&self) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.poll();
        }
    }
    /// Called at VST3 attachment/setFrame. CLAP services poll through its own
    /// host timer. Keep the frame/run-loop reference until unregister completes.
    pub unsafe fn set_frame(&mut self, frame: *mut IPlugFrame) -> bool {
        self.timer = None;
        if frame.is_null() {
            return true;
        }
        let Some(frame) = (unsafe { ComRef::from_raw(frame) }) else {
            return false;
        };
        let Some(run_loop) = frame.to_com_ptr().cast::<IRunLoop>() else {
            return false;
        };
        let tick = ComWrapper::new(Tick {
            inner: Rc::downgrade(&self.inner),
        })
        .to_com_ptr::<ITimerHandler>()
        .unwrap();
        if unsafe { run_loop.registerTimer(tick.as_ptr(), 16) } != kResultOk {
            return false;
        }
        self.timer = Some(Timer { run_loop, tick });
        true
    }
}
