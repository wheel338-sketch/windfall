//! Xlib ABI. Window/Atom/Time are unsigned long, not pointers or uint32_t.
use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void};
pub type Display = c_void;
pub type Xid = c_ulong;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AnyEvent {
    pub kind: c_int,
    pub serial: c_ulong,
    pub sent: c_int,
    pub display: *mut Display,
    pub window: Xid,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ConfigureEvent {
    pub any: AnyEvent,
    pub target: Xid,
    pub x: c_int,
    pub y: c_int,
    pub width: c_int,
    pub height: c_int,
    pub border: c_int,
    pub above: Xid,
    pub override_redirect: c_int,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ClientEvent {
    pub any: AnyEvent,
    pub message_type: Xid,
    pub format: c_int,
    pub data: [c_long; 5],
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DestroyEvent {
    pub any: AnyEvent,
    pub target: Xid,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FocusEvent {
    pub any: AnyEvent,
    pub mode: c_int,
    pub detail: c_int,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct KeyEvent {
    pub any: AnyEvent,
    pub root: Xid,
    pub subwindow: Xid,
    pub time: c_ulong,
    pub x: c_int,
    pub y: c_int,
    pub root_x: c_int,
    pub root_y: c_int,
    pub state: c_uint,
    pub keycode: c_uint,
    pub same_screen: c_int,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub union Event {
    pub kind: c_int,
    pub any: AnyEvent,
    pub configure: ConfigureEvent,
    pub client: ClientEvent,
    pub destroy: DestroyEvent,
    pub focus: FocusEvent,
    pub key: KeyEvent,
    pub pad: [c_long; 24],
}
#[repr(C)]
pub struct SizeHints {
    pub flags: c_long,
    pub x: c_int,
    pub y: c_int,
    pub width: c_int,
    pub height: c_int,
    pub min_width: c_int,
    pub min_height: c_int,
    pub max_width: c_int,
    pub max_height: c_int,
    pub width_inc: c_int,
    pub height_inc: c_int,
    pub min_aspect_x: c_int,
    pub min_aspect_y: c_int,
    pub max_aspect_x: c_int,
    pub max_aspect_y: c_int,
    pub base_width: c_int,
    pub base_height: c_int,
    pub gravity: c_int,
}
#[link(name = "X11")]
unsafe extern "C" {
    pub fn XInitThreads() -> c_int;
    pub fn XOpenDisplay(name: *const c_char) -> *mut Display;
    pub fn XCloseDisplay(display: *mut Display) -> c_int;
    pub fn XDefaultScreen(display: *mut Display) -> c_int;
    pub fn XRootWindow(display: *mut Display, screen: c_int) -> Xid;
    pub fn XCreateSimpleWindow(
        display: *mut Display,
        parent: Xid,
        x: c_int,
        y: c_int,
        width: c_uint,
        height: c_uint,
        border: c_uint,
        border_pixel: c_ulong,
        background: c_ulong,
    ) -> Xid;
    pub fn XDestroyWindow(display: *mut Display, window: Xid) -> c_int;
    pub fn XMapWindow(display: *mut Display, window: Xid) -> c_int;
    pub fn XUnmapWindow(display: *mut Display, window: Xid) -> c_int;
    pub fn XStoreName(display: *mut Display, window: Xid, name: *const c_char) -> c_int;
    pub fn XSelectInput(display: *mut Display, window: Xid, mask: c_long) -> c_int;
    pub fn XInternAtom(display: *mut Display, name: *const c_char, only_if_exists: c_int) -> Xid;
    pub fn XSetWMProtocols(
        display: *mut Display,
        window: Xid,
        protocols: *mut Xid,
        count: c_int,
    ) -> c_int;
    pub fn XSetTransientForHint(display: *mut Display, window: Xid, owner: Xid) -> c_int;
    pub fn XSetWMNormalHints(display: *mut Display, window: Xid, hints: *mut SizeHints);
    pub fn XChangeProperty(
        display: *mut Display,
        window: Xid,
        property: Xid,
        kind: Xid,
        format: c_int,
        mode: c_int,
        data: *const u8,
        count: c_int,
    ) -> c_int;
    pub fn XResizeWindow(
        display: *mut Display,
        window: Xid,
        width: c_uint,
        height: c_uint,
    ) -> c_int;
    pub fn XFlush(display: *mut Display) -> c_int;
    pub fn XPending(display: *mut Display) -> c_int;
    pub fn XNextEvent(display: *mut Display, event: *mut Event) -> c_int;
    pub fn XFilterEvent(event: *mut Event, window: Xid) -> c_int;
    pub fn XSendEvent(
        display: *mut Display,
        window: Xid,
        propagate: c_int,
        mask: c_long,
        event: *mut Event,
    ) -> c_int;
    pub fn XGetGeometry(
        display: *mut Display,
        window: Xid,
        root: *mut Xid,
        x: *mut c_int,
        y: *mut c_int,
        width: *mut c_uint,
        height: *mut c_uint,
        border: *mut c_uint,
        depth: *mut c_uint,
    ) -> c_int;
    pub fn XTranslateCoordinates(
        display: *mut Display,
        source: Xid,
        destination: Xid,
        x: c_int,
        y: c_int,
        destination_x: *mut c_int,
        destination_y: *mut c_int,
        child: *mut Xid,
    ) -> c_int;
    pub fn XResourceManagerString(display: *mut Display) -> *mut c_char;
    pub fn XCreateGC(
        display: *mut Display,
        drawable: Xid,
        mask: c_ulong,
        values: *mut c_void,
    ) -> *mut c_void;
    pub fn XFreeGC(display: *mut Display, gc: *mut c_void) -> c_int;
    pub fn XSetForeground(display: *mut Display, gc: *mut c_void, foreground: c_ulong) -> c_int;
    pub fn XClearWindow(display: *mut Display, window: Xid) -> c_int;
    pub fn XDrawString(
        display: *mut Display,
        window: Xid,
        gc: *mut c_void,
        x: c_int,
        y: c_int,
        text: *const c_char,
        count: c_int,
    ) -> c_int;
    pub fn XDrawRectangle(
        display: *mut Display,
        window: Xid,
        gc: *mut c_void,
        x: c_int,
        y: c_int,
        width: c_uint,
        height: c_uint,
    ) -> c_int;
    pub fn XLookupString(
        event: *mut KeyEvent,
        buffer: *mut c_char,
        size: c_int,
        keysym: *mut c_ulong,
        status: *mut c_void,
    ) -> c_int;
    pub fn XOpenIM(
        display: *mut Display,
        database: *mut c_void,
        resource_name: *mut c_char,
        resource_class: *mut c_char,
    ) -> *mut c_void;
    pub fn XCloseIM(input_method: *mut c_void) -> c_int;
    pub fn XCreateIC(input_method: *mut c_void, ...) -> *mut c_void;
    pub fn XDestroyIC(input_context: *mut c_void);
    pub fn XSetICFocus(input_context: *mut c_void);
    pub fn XUnsetICFocus(input_context: *mut c_void);
    pub fn Xutf8LookupString(
        input_context: *mut c_void,
        event: *mut KeyEvent,
        buffer: *mut c_char,
        bytes: c_int,
        keysym: *mut c_ulong,
        status: *mut c_int,
    ) -> c_int;
    pub fn XSetInputFocus(display: *mut Display, focus: Xid, revert: c_int, time: c_ulong)
    -> c_int;
}
// Checked XCB requests on Xlib's own underlying connection avoid Xlib's
// process-fatal default error handler when a plugin destroys/reparents a child
// between observation and XEmbed notification. Xlib retains event queue ownership.
#[repr(C)]
pub struct Cookie {
    pub sequence: c_uint,
}
#[repr(C)]
pub struct QueryTreeReply {
    pub response: u8,
    pub pad: u8,
    pub sequence: u16,
    pub length: u32,
    pub root: u32,
    pub parent: u32,
    pub children: u16,
    pub padding: [u8; 14],
}
#[repr(C)]
pub struct ClientMessage32 {
    pub response: u8,
    pub format: u8,
    pub sequence: u16,
    pub window: u32,
    pub kind: u32,
    pub data: [u32; 5],
}
#[repr(C)]
pub struct KeyEvent32 {
    pub response: u8,
    pub detail: u8,
    pub sequence: u16,
    pub time: u32,
    pub root: u32,
    pub event: u32,
    pub child: u32,
    pub root_x: i16,
    pub root_y: i16,
    pub x: i16,
    pub y: i16,
    pub state: u16,
    pub same_screen: u8,
    pub padding: u8,
}
#[link(name = "X11-xcb")]
unsafe extern "C" {
    pub fn XGetXCBConnection(display: *mut Display) -> *mut c_void;
}
#[link(name = "xcb")]
unsafe extern "C" {
    pub fn xcb_query_tree(connection: *mut c_void, window: u32) -> Cookie;
    pub fn xcb_query_tree_reply(
        connection: *mut c_void,
        cookie: Cookie,
        error: *mut *mut c_void,
    ) -> *mut QueryTreeReply;
    pub fn xcb_query_tree_children(reply: *const QueryTreeReply) -> *mut u32;
    pub fn xcb_send_event_checked(
        connection: *mut c_void,
        propagate: u8,
        destination: u32,
        mask: u32,
        event: *const c_char,
    ) -> Cookie;
    pub fn xcb_request_check(connection: *mut c_void, cookie: Cookie) -> *mut c_void;
}
unsafe extern "C" {
    pub fn free(pointer: *mut c_void);
}
#[repr(C)]
pub struct Resources {
    pub timestamp: c_ulong,
    pub config_timestamp: c_ulong,
    pub crtc_count: c_int,
    pub crtcs: *mut Xid,
    pub output_count: c_int,
    pub outputs: *mut Xid,
    pub mode_count: c_int,
    pub modes: *mut c_void,
}
#[repr(C)]
pub struct OutputInfo {
    pub timestamp: c_ulong,
    pub crtc: Xid,
    pub name: *mut c_char,
    pub name_length: c_int,
    pub mm_width: c_ulong,
    pub mm_height: c_ulong,
    pub connection: u16,
    pub subpixel: u16,
    pub crtc_count: c_int,
    pub crtcs: *mut Xid,
    pub clone_count: c_int,
    pub clones: *mut Xid,
    pub mode_count: c_int,
    pub preferred: c_int,
    pub modes: *mut Xid,
}
#[repr(C)]
pub struct CrtcInfo {
    pub timestamp: c_ulong,
    pub x: c_int,
    pub y: c_int,
    pub width: c_uint,
    pub height: c_uint,
    pub mode: Xid,
    pub rotation: u16,
    pub output_count: c_int,
    pub outputs: *mut Xid,
    pub rotations: u16,
    pub possible_count: c_int,
    pub possible: *mut Xid,
}
#[link(name = "Xrandr")]
unsafe extern "C" {
    pub fn XRRGetScreenResourcesCurrent(display: *mut Display, root: Xid) -> *mut Resources;
    pub fn XRRFreeScreenResources(resources: *mut Resources);
    pub fn XRRGetOutputInfo(
        display: *mut Display,
        resources: *mut Resources,
        output: Xid,
    ) -> *mut OutputInfo;
    pub fn XRRFreeOutputInfo(info: *mut OutputInfo);
    pub fn XRRGetCrtcInfo(
        display: *mut Display,
        resources: *mut Resources,
        crtc: Xid,
    ) -> *mut CrtcInfo;
    pub fn XRRFreeCrtcInfo(info: *mut CrtcInfo);
}
