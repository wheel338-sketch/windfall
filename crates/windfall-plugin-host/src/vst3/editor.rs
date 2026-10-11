//! Native Windows IPlugView ownership; other platforms are explicit boundaries.
#[cfg(windows)]
use crate::gui::win32::{HostWindow, WindowClient};
use crate::{
    gui::{EditorError, EditorInfo, EditorOptions},
    instance::PluginNotification,
};
#[cfg(windows)]
use std::{cell::Cell, rc::Rc};
#[cfg(windows)]
use vst3::Steinberg::Vst::IEditControllerTrait;
#[cfg(windows)]
use vst3::{Class, ComWrapper, Steinberg::*};
use vst3::{ComPtr, Steinberg::Vst::IEditController};

#[cfg(windows)]
struct Size {
    width: Cell<u32>,
    height: Cell<u32>,
    requested: Cell<bool>,
}
#[cfg(windows)]
struct Frame {
    view: ComPtr<IPlugView>,
    size: Rc<Size>,
    main: usize,
    alive: Cell<bool>,
    resizing: Cell<bool>,
}
#[cfg(windows)]
impl Class for Frame {
    type Interfaces = (IPlugFrame,);
}
#[cfg(windows)]
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
#[cfg(windows)]
fn dimensions(rect: &ViewRect) -> Option<(u32, u32)> {
    let width = rect.right.checked_sub(rect.left)?;
    let height = rect.bottom.checked_sub(rect.top)?;
    ((1..=32768).contains(&width) && (1..=32768).contains(&height))
        .then_some((width as u32, height as u32))
}
#[cfg(windows)]
struct Client {
    view: ComPtr<IPlugView>,
    size: Rc<Size>,
}
#[cfg(windows)]
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
        self.size.width.set(width);
        self.size.height.set(height);
        let mut rect = ViewRect {
            left: 0,
            top: 0,
            right: width as i32,
            bottom: height as i32,
        };
        // SAFETY: window procedure runs on the editor's owning thread.
        unsafe {
            self.view.onSize(&mut rect);
        }
    }
}
pub(super) struct Editor {
    #[cfg(windows)]
    view: ComPtr<IPlugView>,
    #[cfg(windows)]
    frame: ComWrapper<Frame>,
    #[cfg(windows)]
    window: HostWindow,
    #[cfg(windows)]
    size: Rc<Size>,
    #[cfg(windows)]
    resizable: bool,
}
impl Editor {
    pub fn open(
        controller: &ComPtr<IEditController>,
        options: &EditorOptions,
    ) -> Result<Self, EditorError> {
        #[cfg(not(windows))]
        {
            let _ = (controller, options);
            Err(EditorError::UnsupportedPlatform)
        }
        #[cfg(windows)]
        {
            // SAFETY: live controller on its main thread; owned view reference.
            let view = unsafe { ComPtr::from_raw(controller.createView(c"editor".as_ptr())) }
                .ok_or(EditorError::NoEditor)?;
            // SAFETY: static null-terminated SDK platform string.
            if unsafe { view.isPlatformTypeSupported(c"HWND".as_ptr()) } != kResultOk {
                return Err(EditorError::UnsupportedPlatform);
            }
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
            });
            let interface = frame.to_com_ptr::<IPlugFrame>().expect("frame");
            // SAFETY: all editor objects stay alive until removed/reset-frame.
            unsafe {
                if view.setFrame(interface.as_ptr()) != kResultOk {
                    return Err(EditorError::Failed("editor refused its frame".into()));
                }
                if view.attached(window.handle(), c"HWND".as_ptr()) != kResultOk {
                    view.setFrame(std::ptr::null_mut());
                    return Err(EditorError::Failed("editor attach refused".into()));
                }
            }
            window.show(options.take_focus);
            Ok(Self {
                view,
                frame,
                window,
                size,
                resizable,
            })
        }
    }
    pub fn info(&self) -> EditorInfo {
        #[cfg(windows)]
        {
            EditorInfo {
                window: Some(self.window.handle() as usize),
                width: self.size.width.get(),
                height: self.size.height.get(),
                resizable: self.resizable,
            }
        }
        #[cfg(not(windows))]
        {
            unreachable!("no editor can be created on this platform")
        }
    }
    pub fn idle(&mut self, notify: &mut dyn FnMut(PluginNotification)) -> bool {
        #[cfg(windows)]
        {
            if self.size.requested.replace(false) {
                let width = self.size.width.get();
                let height = self.size.height.get();
                self.window.set_client_size(width, height);
                notify(PluginNotification::EditorResized { width, height });
            }
            self.window.take_close_request()
        }
        #[cfg(not(windows))]
        {
            let _ = notify;
            false
        }
    }
}
#[cfg(windows)]
impl Drop for Editor {
    fn drop(&mut self) {
        self.frame.alive.set(false);
        // SAFETY: main-thread removal before the native parent is destroyed.
        unsafe {
            self.view.removed();
            self.view.setFrame(std::ptr::null_mut());
        }
    }
}
