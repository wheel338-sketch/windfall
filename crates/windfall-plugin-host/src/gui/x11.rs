//! XEmbed container. Each editor owns its Xlib connection on the GUI owner;
//! plugins use their own connections and VST3 exposes these via IRunLoop.
use super::{
    lifecycle::{Backend, Observation},
    x11_ffi::*,
};
use std::{
    cell::{Cell, RefCell},
    ffi::{CStr, CString, c_void},
    ptr,
    sync::OnceLock,
};
pub(crate) struct Window {
    display: *mut Display,
    window: Xid,
    root: Xid,
    delete: Xid,
    protocols: Xid,
    size: Cell<(u32, u32)>,
    closed: Cell<bool>,
    destroyed: Cell<bool>,
    children: RefCell<Vec<u32>>,
    focused: Cell<bool>,
    resizable: bool,
}
impl Window {
    pub fn create(
        title: &str,
        width: u32,
        height: u32,
        resizable: bool,
        position: Option<(i32, i32)>,
    ) -> Result<Self, String> {
        let title = CString::new(title).map_err(|_| "invalid window title")?;
        static THREADS: OnceLock<bool> = OnceLock::new();
        if !*THREADS.get_or_init(|| unsafe { XInitThreads() != 0 }) {
            return Err("Xlib thread initialization failed".into());
        }
        unsafe {
            let display = XOpenDisplay(ptr::null());
            if display.is_null() {
                return Err(
                    "X11 display unavailable (an XWayland display is required on Wayland)".into(),
                );
            }
            let root = XRootWindow(display, XDefaultScreen(display));
            let (x, y) = position.unwrap_or((100, 100));
            let window = XCreateSimpleWindow(display, root, x, y, width, height, 0, 0, 0x202020);
            if window == 0 {
                XCloseDisplay(display);
                return Err("X11 refused the editor container".into());
            }
            XStoreName(display, window, title.as_ptr());
            XSelectInput(
                display,
                window,
                (1 << 0) | (1 << 1) | (1 << 17) | (1 << 19) | (1 << 21),
            ); // Keys/Structure/SubstructureNotify/Focus
            let mut delete = XInternAtom(display, c"WM_DELETE_WINDOW".as_ptr(), 0);
            let protocols = XInternAtom(display, c"WM_PROTOCOLS".as_ptr(), 0);
            XSetWMProtocols(display, window, &mut delete, 1);
            if !resizable {
                let mut hints: SizeHints = std::mem::zeroed();
                hints.flags = (1 << 4) | (1 << 5);
                hints.min_width = width as i32;
                hints.max_width = width as i32;
                hints.min_height = height as i32;
                hints.max_height = height as i32;
                XSetWMNormalHints(display, window, &mut hints);
            }
            XFlush(display);
            Ok(Self {
                display,
                window,
                root,
                delete,
                protocols,
                size: Cell::new((width, height)),
                closed: Cell::new(false),
                destroyed: Cell::new(false),
                children: RefCell::new(Vec::new()),
                focused: Cell::new(false),
                resizable,
            })
        }
    }
    pub fn handle(&self) -> *mut c_void {
        self.window as usize as *mut c_void
    }
    pub fn show(&self, focus: bool) {
        unsafe {
            XMapWindow(self.display, self.window);
            // _NET_ACTIVE_WINDOW lets the WM apply its focus-stealing policy.
            if focus {
                let mut event = Event {
                    client: ClientEvent {
                        any: AnyEvent {
                            kind: 33,
                            serial: 0,
                            sent: 1,
                            display: self.display,
                            window: self.window,
                        },
                        message_type: XInternAtom(self.display, c"_NET_ACTIVE_WINDOW".as_ptr(), 0),
                        format: 32,
                        data: [1, 0, 0, 0, 0],
                    },
                };
                XSendEvent(
                    self.display,
                    self.root,
                    0,
                    (1 << 19) | (1 << 20),
                    &mut event,
                );
            }
            XFlush(self.display);
        }
    }
    pub fn set_owner(&self, owner: Option<usize>) -> Result<(), String> {
        if let Some(owner) = owner {
            if owner == 0 {
                return Err("Missing X11 owner window id".into());
            }
            unsafe {
                XSetTransientForHint(self.display, self.window, owner as Xid);
                XFlush(self.display);
            }
        }
        Ok(())
    }
    fn embed_message(&self, child: u32, message: u32, detail: u32) {
        unsafe {
            let connection = XGetXCBConnection(self.display);
            if connection.is_null() {
                return;
            }
            let event = ClientMessage32 {
                response: 33,
                format: 32,
                sequence: 0,
                window: child,
                kind: XInternAtom(self.display, c"_XEMBED".as_ptr(), 0) as u32,
                data: [
                    0,
                    message,
                    detail,
                    if message == 0 { self.window as u32 } else { 0 },
                    0,
                ],
            };
            let cookie = xcb_send_event_checked(
                connection,
                0,
                child,
                0,
                (&event as *const ClientMessage32).cast(),
            );
            let error = xcb_request_check(connection, cookie);
            if !error.is_null() {
                free(error);
            }
        }
    }
    fn embed_children(&self) {
        unsafe {
            let connection = XGetXCBConnection(self.display);
            if connection.is_null() {
                return;
            }
            let mut error = ptr::null_mut();
            let reply = xcb_query_tree_reply(
                connection,
                xcb_query_tree(connection, self.window as u32),
                &mut error,
            );
            if !error.is_null() {
                free(error);
            }
            if reply.is_null() {
                return;
            }
            let count = usize::from((*reply).children);
            let children =
                std::slice::from_raw_parts(xcb_query_tree_children(reply), count.min(256)).to_vec();
            free(reply.cast());
            let new = children
                .iter()
                .copied()
                .filter(|child| !self.children.borrow().contains(child))
                .collect::<Vec<_>>();
            *self.children.borrow_mut() = children;
            for child in new {
                self.embed_message(child, 0, 0); // XEMBED_EMBEDDED_NOTIFY, version 0
                self.embed_message(child, if self.focused.get() { 1 } else { 2 }, 0);
                if self.focused.get() {
                    self.embed_message(child, 4, 0);
                }
            }
        }
    }
    fn forward_key(&self, key: KeyEvent) {
        let Some(child) = self.children.borrow().first().copied() else {
            return;
        };
        unsafe {
            let connection = XGetXCBConnection(self.display);
            if connection.is_null() {
                return;
            }
            // The container is XEmbed's keyboard proxy. Forward its core key
            // events to the plugin rather than requiring the plugin to steal
            // server focus from the DAW. Native child focus needs no forwarding.
            let event = KeyEvent32 {
                response: key.any.kind as u8,
                detail: key.keycode as u8,
                sequence: 0,
                time: key.time as u32,
                root: key.root as u32,
                event: child,
                child: 0,
                root_x: key.root_x as i16,
                root_y: key.root_y as i16,
                x: key.x as i16,
                y: key.y as i16,
                state: key.state as u16,
                same_screen: u8::from(key.same_screen != 0),
                padding: 0,
            };
            let mask = if key.any.kind == 2 { 1 } else { 2 };
            let cookie = xcb_send_event_checked(
                connection,
                0,
                child,
                mask,
                (&event as *const KeyEvent32).cast(),
            );
            let error = xcb_request_check(connection, cookie);
            if !error.is_null() {
                free(error);
            }
        }
    }
    fn monitor(&self) -> f64 {
        unsafe {
            let mut x = 0;
            let mut y = 0;
            let mut child = 0;
            XTranslateCoordinates(
                self.display,
                self.window,
                self.root,
                0,
                0,
                &mut x,
                &mut y,
                &mut child,
            );
            x += self.size.get().0 as i32 / 2;
            y += self.size.get().1 as i32 / 2;
            let resources = XRRGetScreenResourcesCurrent(self.display, self.root);
            let mut density = None;
            if !resources.is_null() {
                if (0..=256).contains(&(*resources).output_count) && !(*resources).outputs.is_null()
                {
                    for output in std::slice::from_raw_parts(
                        (*resources).outputs,
                        (*resources).output_count as usize,
                    ) {
                        let info = XRRGetOutputInfo(self.display, resources, *output);
                        if info.is_null() {
                            continue;
                        }
                        if (*info).connection == 0 && (*info).crtc != 0 && (*info).mm_width > 0 {
                            let crtc = XRRGetCrtcInfo(self.display, resources, (*info).crtc);
                            if !crtc.is_null() {
                                if x >= (*crtc).x
                                    && y >= (*crtc).y
                                    && x < (*crtc).x + (*crtc).width as i32
                                    && y < (*crtc).y + (*crtc).height as i32
                                {
                                    let rotated = (*crtc).rotation & (2 | 8) != 0;
                                    let mm = if rotated {
                                        (*info).mm_height
                                    } else {
                                        (*info).mm_width
                                    };
                                    density = super::scaling::physical_monitor_scale(
                                        (*crtc).width,
                                        mm,
                                    );
                                }
                                XRRFreeCrtcInfo(crtc);
                            }
                        }
                        XRRFreeOutputInfo(info);
                    }
                }
                XRRFreeScreenResources(resources);
            }
            // Xft.dpi is the user's global override. Otherwise moving the editor
            // across RandR outputs updates the physical monitor density.
            let resource = XResourceManagerString(self.display);
            if !resource.is_null()
                && let Some(scale) =
                    super::scaling::xft_scale(&CStr::from_ptr(resource).to_string_lossy())
            {
                return scale;
            }
            density
                .filter(|scale| scale.is_finite() && *scale > 0.0)
                .unwrap_or(1.0)
                .clamp(0.5, 8.0)
        }
    }
}
impl Backend for Window {
    fn observe(&self) -> Observation {
        unsafe {
            // A DestroyNotify means the WM/server has destroyed the native
            // parent. Do not query/resize that XID again (BadWindow is fatal
            // under Xlib's default error handler).
            for _ in 0..256 {
                if XPending(self.display) == 0 {
                    break;
                }
                let mut event = Event { pad: [0; 24] };
                XNextEvent(self.display, &mut event);
                if event.any.window != self.window {
                    continue;
                }
                match event.kind {
                    2 | 3 => self.forward_key(event.key),
                    22 => {
                        let e = event.configure;
                        if e.target == self.window && e.width > 0 && e.height > 0 {
                            self.size.set((e.width as u32, e.height as u32));
                        }
                    }
                    17 => {
                        if event.destroy.target == self.window {
                            self.destroyed.set(true);
                            self.closed.set(true);
                        }
                    }
                    9 | 10 => {
                        let focus = event.focus;
                        // Moving server focus into a child or a temporary grab
                        // must not deactivate the containing plugin window.
                        if focus.mode != 0 || focus.detail == 2 {
                            continue;
                        }
                        let focused = event.kind == 9;
                        self.focused.set(focused);
                        for child in self.children.borrow().iter().copied() {
                            self.embed_message(child, if focused { 1 } else { 2 }, 0);
                            self.embed_message(child, if focused { 4 } else { 5 }, 0);
                        }
                    }
                    33 => {
                        let e = event.client;
                        if e.message_type == self.protocols
                            && e.format == 32
                            && e.data[0] as Xid == self.delete
                        {
                            self.closed.set(true);
                        }
                        if e.message_type == XInternAtom(self.display, c"_XEMBED".as_ptr(), 0)
                            && e.format == 32
                            && e.data[1] == 3
                        {
                            for child in self.children.borrow().iter().copied() {
                                self.embed_message(child, 4, 0);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if !self.closed.get() {
            self.embed_children();
        }
        Observation {
            size: self.size.get(),
            monitor: if self.closed.get() {
                1.0
            } else {
                self.monitor()
            },
            closed: self.closed.get(),
        }
    }
    fn resize(&self, size: (u32, u32)) {
        if self.closed.get() {
            return;
        }
        self.size.set(size);
        unsafe {
            if !self.resizable {
                let mut hints: SizeHints = std::mem::zeroed();
                hints.flags = (1 << 4) | (1 << 5);
                hints.min_width = size.0 as i32;
                hints.max_width = size.0 as i32;
                hints.min_height = size.1 as i32;
                hints.max_height = size.1 as i32;
                XSetWMNormalHints(self.display, self.window, &mut hints);
            }
            XResizeWindow(self.display, self.window, size.0, size.1);
            XFlush(self.display);
        }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            if !self.destroyed.get() {
                XDestroyWindow(self.display, self.window);
            }
            XCloseDisplay(self.display);
        }
    }
}
