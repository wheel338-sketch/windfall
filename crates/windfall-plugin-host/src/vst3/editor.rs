//! Owner-thread IPlugView attachment, with HWND, NSView and XEmbed parents.
#[cfg(target_os = "linux")]
pub(super) mod run_loop;
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn platform() -> &'static std::ffi::CStr {
    #[cfg(windows)]
    {
        c"HWND"
    }
    #[cfg(target_os = "macos")]
    {
        c"NSView"
    }
    #[cfg(target_os = "linux")]
    {
        c"X11EmbedWindowID"
    }
}
#[cfg(any(target_os = "macos", target_os = "linux"))]
use crate::gui::native::{HostWindow, WindowClient};
#[cfg(windows)]
use crate::gui::win32::{HostWindow, WindowClient};
use crate::{
    gui::{EditorError, EditorInfo, EditorOptions},
    instance::PluginNotification,
};
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
use std::{cell::Cell, rc::Rc};
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
use vst3::Steinberg::Vst::IEditControllerTrait;
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
use vst3::{Class, ComWrapper, Steinberg::*};
use vst3::{ComPtr, Steinberg::Vst::IEditController};

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
struct Size {
    width: Cell<u32>,
    height: Cell<u32>,
    requested: Cell<bool>,
}
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
struct Frame {
    view: ComPtr<IPlugView>,
    size: Rc<Size>,
    main: usize,
    alive: Cell<bool>,
    resizing: Cell<bool>,
    #[cfg(target_os = "linux")]
    run_loop: run_loop::RunLoop,
}
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
impl Class for Frame {
    #[cfg(not(target_os = "linux"))]
    type Interfaces = (IPlugFrame,);
    #[cfg(target_os = "linux")]
    type Interfaces = (IPlugFrame, vst3::Steinberg::Linux::IRunLoop);
}
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
#[allow(non_snake_case)]
impl IPlugFrameTrait for Frame {
    unsafe fn resizeView(&self, view: *mut IPlugView, rect: *mut ViewRect) -> tresult {
        if self.main != super::handlers::token()
            || !self.alive.get()
            || view != self.view.as_ptr()
            || rect.is_null()
            || self.resizing.get()
        {
            return kInvalidArgument;
        }
        // SAFETY: plugin passes a live SDK rectangle for this synchronous call.
        let rect = unsafe { &mut *rect };
        let Some((width, height)) = dimensions(rect) else {
            return kInvalidArgument;
        };
        self.size.width.set(width);
        self.size.height.set(height);
        self.size.requested.set(true);
        // SAFETY: main-thread editor call; live view and caller rectangle.
        self.resizing.set(true);
        let result = unsafe { self.view.onSize(rect) };
        self.resizing.set(false);
        result
    }
}
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
fn dimensions(rect: &ViewRect) -> Option<(u32, u32)> {
    let width = rect.right.checked_sub(rect.left)?;
    let height = rect.bottom.checked_sub(rect.top)?;
    ((1..=32768).contains(&width) && (1..=32768).contains(&height))
        .then_some((width as u32, height as u32))
}
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
struct Client {
    view: ComPtr<IPlugView>,
    size: Rc<Size>,
}
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
impl WindowClient for Client {
    fn adjust_size(&self, width: u32, height: u32) -> (u32, u32) {
        let mut rect = ViewRect {
            left: 0,
            top: 0,
            right: width.min(32768) as i32,
            bottom: height.min(32768) as i32,
        };
        // SAFETY: main-thread native resize, bounded sized rectangle.
        unsafe {
            self.view.checkSizeConstraint(&mut rect);
        }
        dimensions(&rect).unwrap_or((self.size.width.get(), self.size.height.get()))
    }
    fn resized(&self, width: u32, height: u32) {
        #[cfg(windows)]
        {
            self.size.width.set(width);
            self.size.height.set(height);
        }
        let mut rect = ViewRect {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        };
        // SAFETY: window procedure runs on the editor's owning thread.
        unsafe {
            #[cfg(windows)]
            self.view.onSize(&mut rect);
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            if self.view.onSize(&mut rect) == kResultOk {
                self.size.width.set(width);
                self.size.height.set(height);
            }
        }
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn accepted_size(&self) -> (u32, u32) {
        (self.size.width.get(), self.size.height.get())
    }
}
pub(super) struct Editor {
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    view: ComPtr<IPlugView>,
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    frame: ComWrapper<Frame>,
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    window: HostWindow,
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    size: Rc<Size>,
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    resizable: bool,
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    application: f64,
}
impl Editor {
    pub fn set_scale(&mut self, application: f64) -> Result<EditorInfo, EditorError> {
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        {
            let factor =
                crate::gui::scaling::content_scale(application, self.window.monitor_scale())?;
            let support = self
                .view
                .cast::<IPlugViewContentScaleSupport>()
                .ok_or(EditorError::UnsupportedScale)?;
            // SAFETY: live view on its owning GUI thread. Its frame remains
            // installed throughout synchronous scale/resize callbacks.
            if unsafe { support.setContentScaleFactor(factor as f32) } != kResultOk {
                return Err(EditorError::UnsupportedScale);
            }
            let mut rect = ViewRect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            if unsafe { self.view.getSize(&mut rect) } != kResultOk {
                return Err(EditorError::Failed(
                    "scaled editor did not supply a size".into(),
                ));
            }
            let (width, height) = dimensions(&rect)
                .ok_or_else(|| EditorError::Failed("invalid scaled editor dimensions".into()))?;
            self.size.width.set(width);
            self.size.height.set(height);
            self.window.set_client_size(width, height);
            // SAFETY: matching bounded rectangle on the same GUI thread.
            unsafe {
                self.view.onSize(&mut rect);
            }
            self.application = application;
            Ok(self.info())
        }
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        {
            let _ = application;
            Err(EditorError::UnsupportedScale)
        }
    }
    pub fn open(
        controller: &ComPtr<IEditController>,
        options: &EditorOptions,
    ) -> Result<Self, EditorError> {
        #[cfg(target_os = "macos")]
        if !crate::gui::cocoa_ffi::is_main() {
            return Err(EditorError::Failed(
                "Cocoa editor requires the application main thread".into(),
            ));
        }
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        {
            let _ = (controller, options);
            Err(EditorError::UnsupportedPlatform)
        }
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        {
            // SAFETY: live controller on its main thread; owned view reference.
            let view = unsafe { ComPtr::from_raw(controller.createView(c"editor".as_ptr())) }
                .ok_or(EditorError::NoEditor)?;
            // SAFETY: static null-terminated SDK platform string.
            if unsafe { view.isPlatformTypeSupported(platform().as_ptr()) } != kResultOk {
                return Err(EditorError::UnsupportedPlatform);
            }
            #[cfg(windows)]
            {
                let scale = crate::gui::win32::system_scale() * options.scale;
                if let Some(support) = view.cast::<IPlugViewContentScaleSupport>() {
                    if unsafe { support.setContentScaleFactor(scale as f32) } != kResultOk
                        && options.scale != 1.0
                    {
                        return Err(EditorError::Failed("VST3 editor refused scaling".into()));
                    }
                } else if options.scale != 1.0 {
                    return Err(EditorError::Failed(
                        "VST3 editor has no scale capability".into(),
                    ));
                }
            }
            let mut rect = ViewRect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            if unsafe { view.getSize(&mut rect) } != kResultOk {
                return Err(EditorError::Failed("editor did not supply a size".into()));
            }
            let (width, height) = dimensions(&rect)
                .ok_or_else(|| EditorError::Failed("invalid editor dimensions".into()))?;
            let resizable = unsafe { view.canResize() } == kResultOk;
            let size = Rc::new(Size {
                width: Cell::new(width),
                height: Cell::new(height),
                requested: Cell::new(false),
            });
            let window = HostWindow::create(
                &options.title,
                width,
                height,
                resizable,
                options.position,
                Box::new(Client {
                    view: Clone::clone(&view),
                    size: size.clone(),
                }),
            )
            .map_err(EditorError::Failed)?;
            window
                .set_owner(if options.detached {
                    None
                } else {
                    options.owner_window
                })
                .map_err(EditorError::Failed)?;
            let frame = ComWrapper::new(Frame {
                view: Clone::clone(&view),
                size: size.clone(),
                main: super::handlers::token(),
                alive: Cell::new(true),
                resizing: Cell::new(false),
                #[cfg(target_os = "linux")]
                run_loop: run_loop::RunLoop::new(),
            });
            let interface = frame.to_com_ptr::<IPlugFrame>().expect("frame");
            // SAFETY: all editor objects stay alive until removed/reset-frame.
            unsafe {
                // Fixed-size views may leave frame callbacks unimplemented.
                // Like the SDK editor host, let attachment determine whether
                // the editor can open; retain the frame through removal either way.
                view.setFrame(interface.as_ptr());
                if view.attached(window.handle(), platform().as_ptr()) != kResultOk {
                    view.setFrame(std::ptr::null_mut());
                    return Err(EditorError::Failed("editor attach refused".into()));
                }
            }
            window.show(options.take_focus);
            let mut editor = Self {
                view,
                frame,
                window,
                size,
                resizable,
                application: options.scale,
            };
            // Preserve editors that draw at their own default DPI even when
            // they expose a scale interface but refuse negotiation.
            if (editor.view.cast::<IPlugViewContentScaleSupport>().is_some()
                || options.scale != 1.0)
                && let Err(error) = editor.set_scale(options.scale)
                && options.scale != 1.0
            {
                return Err(error);
            }
            Ok(editor)
        }
    }
    pub fn info(&self) -> EditorInfo {
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        {
            EditorInfo {
                window: Some(self.window.handle() as usize),
                width: self.size.width.get(),
                height: self.size.height.get(),
                resizable: self.resizable,
            }
        }
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        {
            unreachable!("no editor can be created on this platform")
        }
    }
    pub fn idle(&mut self, notify: &mut dyn FnMut(PluginNotification)) -> bool {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        if self.window.take_close_request() {
            return true;
        }
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        {
            if let Some(monitor) = self.window.take_monitor_scale() {
                let before = self.info();
                match self.set_scale(self.application) {
                    Ok(info) => notify(PluginNotification::EditorResized {
                        width: info.width,
                        height: info.height,
                    }),
                    Err(_) => {
                        self.size.width.set(before.width);
                        self.size.height.set(before.height);
                        self.size.requested.set(false);
                        self.window.set_client_size(before.width, before.height);
                        notify(PluginNotification::EditorScaleRefused {
                            application: self.application,
                            monitor,
                        });
                    }
                }
            }
            if self.size.requested.replace(false) {
                let width = self.size.width.get();
                let height = self.size.height.get();
                self.window.set_client_size(width, height);
                notify(PluginNotification::EditorResized { width, height });
            }
            self.window.take_close_request()
        }
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        {
            let _ = notify;
            false
        }
    }
}
#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
impl Drop for Editor {
    fn drop(&mut self) {
        self.frame.alive.set(false);
        // SAFETY: main-thread removal before the native parent is destroyed.
        unsafe {
            self.view.removed();
            self.view.setFrame(std::ptr::null_mut());
        }
        #[cfg(target_os = "linux")]
        self.frame.run_loop.clear();
    }
}
