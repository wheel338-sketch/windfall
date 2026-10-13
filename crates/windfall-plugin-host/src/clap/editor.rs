//! A CLAP plugin's editor: the calls of the `gui` extension, in the order
//! the specification wants them.

use std::sync::atomic::Ordering;

use clack_extensions::gui::{GuiApiType, GuiConfiguration, PluginGui};
use clack_host::prelude::*;

use super::handlers::WindfallHost;
use crate::gui::{EditorError, EditorInfo, EditorOptions};

/// An open editor.
pub(crate) struct Editor {
    gui: PluginGui,
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    window: Option<embedded::Embedded>,
    info: EditorInfo,
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    application: f64,
}

fn failed(what: &str) -> impl Fn(clack_extensions::gui::GuiError) -> EditorError + '_ {
    move |error| EditorError::Failed(format!("{what}: {error}"))
}

impl Editor {
    /// Opens the editor: inside a window of the host's if the plugin can
    /// be embedded and the platform has a host window, otherwise as a
    /// window the plugin makes itself.
    pub fn open(
        instance: &mut PluginInstance<WindfallHost>,
        gui: PluginGui,
        options: &EditorOptions,
    ) -> Result<Self, EditorError> {
        let Some(api) = GuiApiType::default_for_current_platform() else {
            return Err(EditorError::UnsupportedPlatform);
        };
        #[cfg(target_os = "macos")]
        if !crate::gui::cocoa_ffi::is_main() {
            return Err(EditorError::Failed(
                "Cocoa editor requires the application main thread".into(),
            ));
        }
        let embedded = GuiConfiguration {
            api_type: api,
            is_floating: false,
        };
        let floating = GuiConfiguration {
            api_type: api,
            is_floating: true,
        };
        let (can_embed, can_float) = {
            let plugin = instance.plugin_handle();
            (
                gui.is_api_supported(&plugin, embedded),
                gui.is_api_supported(&plugin, floating),
            )
        };

        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        if can_embed {
            let (window, info) = embedded::open(instance, gui, embedded, options)?;
            instance.access_shared_handler(|shared| {
                shared.editor_open.store(true, Ordering::Release);
            });
            let mut editor = Self {
                gui,
                window: Some(window),
                info,
                application: options.scale,
            };
            if let Err(error) = editor.set_scale(instance, options.scale)
                && options.scale != 1.0
            {
                editor.close(instance);
                return Err(error);
            }
            return Ok(editor);
        }

        if !options.detached {
            return Err(EditorError::Failed(
                "this plugin only supplies a detached editor".into(),
            ));
        }
        if !can_float {
            return Err(if can_embed {
                EditorError::UnsupportedPlatform
            } else {
                EditorError::NoEditor
            });
        }
        let plugin = instance.plugin_handle();
        gui.create(&plugin, floating)
            .map_err(failed("the plugin could not create its editor"))?;
        if let Ok(title) = std::ffi::CString::new(options.title.as_str()) {
            gui.suggest_title(&plugin, &title);
        }
        if let Err(error) = gui.show(&plugin) {
            gui.destroy(&plugin);
            return Err(failed("the plugin could not show its editor")(error));
        }
        instance.access_shared_handler(|shared| {
            shared.editor_open.store(true, Ordering::Release);
        });
        Ok(Self {
            gui,
            #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
            window: None,
            #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
            application: options.scale,
            info: EditorInfo {
                window: None,
                width: 0,
                height: 0,
                resizable: false,
            },
        })
    }

    pub fn info(&self) -> EditorInfo {
        self.info
    }

    pub fn set_scale(
        &mut self,
        instance: &mut PluginInstance<WindfallHost>,
        application: f64,
    ) -> Result<EditorInfo, EditorError> {
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        {
            let window = self.window.as_ref().ok_or(EditorError::UnsupportedScale)?;
            let factor =
                crate::gui::scaling::content_scale(application, window.window.monitor_scale())?;
            let plugin = instance.plugin_handle();
            #[cfg(not(target_os = "macos"))]
            self.gui
                .set_scale(&plugin, factor)
                .map_err(|_| EditorError::UnsupportedScale)?;
            #[cfg(target_os = "macos")]
            {
                // CLAP Cocoa dimensions are points and set_scale must not be
                // called. AppKit applies backing DPI; only application zoom is
                // applied through the parent view's bounds transform.
                let _ = factor;
            }
            let size = self
                .gui
                .get_size(&plugin)
                .ok_or_else(|| EditorError::Failed("scaled editor did not supply a size".into()))?;
            if !(1..=32768).contains(&size.width) || !(1..=32768).contains(&size.height) {
                return Err(EditorError::Failed(
                    "invalid scaled editor dimensions".into(),
                ));
            }
            #[cfg(target_os = "macos")]
            window.window.set_application_scale(application)?;
            self.apply_requested_size(size.width, size.height);
            self.application = application;
            Ok(self.info)
        }
        #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
        {
            let _ = (instance, application);
            Err(EditorError::UnsupportedScale)
        }
    }

    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    pub fn poll_monitor_scale(
        &mut self,
        instance: &mut PluginInstance<WindfallHost>,
    ) -> Option<(f64, f64, Result<EditorInfo, EditorError>)> {
        let monitor = self.window.as_ref()?.window.take_monitor_scale()?;
        let before = self.info;
        let result = self.set_scale(instance, self.application);
        if result.is_err() {
            self.apply_requested_size(before.width, before.height);
        }
        Some((self.application, monitor, result))
    }

    /// True once after the user asked to close the host's window.
    pub fn take_close_request(&self) -> bool {
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        if let Some(window) = &self.window {
            return window.window.take_close_request();
        }
        false
    }

    /// Gives the editor the size the plugin asked for. Returns the new size
    /// if it changed.
    pub fn apply_requested_size(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        if !(1..=32768).contains(&width) || !(1..=32768).contains(&height) {
            return None;
        }
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        if let Some(window) = &self.window {
            window.window.set_client_size(width, height);
            window.size.set((width, height));
        }
        self.note_size(width, height)
    }

    /// Picks up a size the user dragged the window to. Returns the new size
    /// if it changed.
    pub fn poll_size(&mut self) -> Option<(u32, u32)> {
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        if let Some(window) = &self.window {
            let (width, height) = window.size.get();
            return self.note_size(width, height);
        }
        None
    }

    fn note_size(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        if (self.info.width, self.info.height) == (width, height) {
            return None;
        }
        self.info.width = width;
        self.info.height = height;
        Some((width, height))
    }

    /// Closes the editor: the plugin's part first, then the host's window,
    /// which is the order CLAP asks for.
    pub fn close(self, instance: &mut PluginInstance<WindfallHost>) {
        instance.access_shared_handler(|shared| {
            shared.editor_open.store(false, Ordering::Release);
        });
        let plugin = instance.plugin_handle();
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let gui = self.gui;
            crate::gui::lifecycle::close_plugin_then_parent(
                || {
                    let _ = gui.hide(&plugin);
                },
                || gui.destroy(&plugin),
                || drop(self),
            );
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let _ = self.gui.hide(&plugin);
            self.gui.destroy(&plugin);
        }
    }
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
mod embedded {
    //! The editor inside a host window. The window procedure has to ask
    //! the plugin about sizes while the user drags the frame, at a moment
    //! when no reference to the instance can be had. It calls the plugin's
    //! `gui` functions through raw pointers instead, and that is the
    //! `unsafe` in this module.

    use std::cell::Cell;
    use std::rc::Rc;

    #[cfg(windows)]
    use clack_extensions::gui::Window;
    use clack_extensions::gui::{GuiConfiguration, GuiSize, PluginGui};
    use clack_host::prelude::*;
    use clap_sys::ext::gui::{CLAP_EXT_GUI, clap_plugin_gui};
    use clap_sys::plugin::clap_plugin;

    use super::super::handlers::WindfallHost;
    use super::failed;
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    use crate::gui::native::{HostWindow, WindowClient};
    #[cfg(windows)]
    use crate::gui::win32::{HostWindow, WindowClient, system_scale};
    use crate::gui::{EditorError, EditorInfo, EditorOptions};

    /// The size given to an editor that does not say how big it is.
    const FALLBACK_SIZE: GuiSize = GuiSize {
        width: 640,
        height: 480,
    };

    pub(crate) struct Embedded {
        pub window: HostWindow,
        /// The editor's size as the window procedure last set it.
        pub size: Rc<Cell<(u32, u32)>>,
    }

    /// The plugin's `gui` functions, for the window procedure.
    struct Client {
        plugin: *const clap_plugin,
        gui: *const clap_plugin_gui,
        size: Rc<Cell<(u32, u32)>>,
    }

    impl WindowClient for Client {
        fn adjust_size(&self, width: u32, height: u32) -> (u32, u32) {
            let (mut width, mut height) = (width, height);
            // SAFETY: both pointers are the plugin's own and stay valid
            // until the editor is closed, which destroys the window, and
            // with it this client, first. This is the main thread, where
            // CLAP wants the call.
            unsafe {
                if let Some(adjust_size) = (*self.gui).adjust_size {
                    #[cfg(any(target_os = "macos", target_os = "linux"))]
                    if !adjust_size(self.plugin, &mut width, &mut height) {
                        return self.size.get();
                    }
                    #[cfg(windows)]
                    adjust_size(self.plugin, &mut width, &mut height);
                }
            }
            (width, height)
        }

        fn resized(&self, width: u32, height: u32) {
            let (width, height) = self.adjust_size(width, height);
            // SAFETY: as in `adjust_size`.
            unsafe {
                if let Some(set_size) = (*self.gui).set_size {
                    #[cfg(any(target_os = "macos", target_os = "linux"))]
                    if !set_size(self.plugin, width, height) {
                        return;
                    }
                    #[cfg(windows)]
                    set_size(self.plugin, width, height);
                }
            }
            self.size.set((width, height));
        }
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        fn accepted_size(&self) -> (u32, u32) {
            self.size.get()
        }
    }

    pub(crate) fn open(
        instance: &mut PluginInstance<WindfallHost>,
        gui: PluginGui,
        configuration: GuiConfiguration,
        options: &EditorOptions,
    ) -> Result<(Embedded, EditorInfo), EditorError> {
        let raw_plugin: *const clap_plugin = instance.raw_instance();
        // SAFETY: the plugin is alive, and asking for an extension is
        // allowed on the main thread at any time.
        let raw_gui = unsafe {
            (*raw_plugin).get_extension.map_or(std::ptr::null(), |get| {
                get(raw_plugin, CLAP_EXT_GUI.as_ptr())
            })
        } as *const clap_plugin_gui;
        if raw_gui.is_null() {
            return Err(EditorError::NoEditor);
        }

        let plugin = instance.plugin_handle();
        gui.create(&plugin, configuration)
            .map_err(failed("the plugin could not create its editor"))?;
        // A plugin that refuses the scale reads it from the system itself.
        #[cfg(windows)]
        if let Err(error) = gui.set_scale(&plugin, system_scale() * options.scale)
            && options.scale != 1.0
        {
            gui.destroy(&plugin);
            return Err(failed("plugin refused editor scaling")(error));
        }
        let size = gui.get_size(&plugin).unwrap_or(FALLBACK_SIZE);
        if !(1..=32768).contains(&size.width) || !(1..=32768).contains(&size.height) {
            gui.destroy(&plugin);
            return Err(EditorError::Failed("invalid editor dimensions".into()));
        }
        let resizable = gui.can_resize(&plugin);

        let shared_size = Rc::new(Cell::new((size.width, size.height)));
        let client = Client {
            plugin: raw_plugin,
            gui: raw_gui,
            size: Rc::clone(&shared_size),
        };
        let window = HostWindow::create(
            &options.title,
            size.width,
            size.height,
            resizable,
            options.position,
            Box::new(client),
        )
        .map_err(|error| {
            gui.destroy(&plugin);
            EditorError::Failed(error)
        })?;

        if let Err(error) = window.set_owner(if options.detached {
            None
        } else {
            options.owner_window
        }) {
            gui.destroy(&plugin);
            return Err(EditorError::Failed(error));
        }
        // SAFETY: the window outlives the plugin's editor: `Editor::close`
        // destroys the plugin's part before the window is dropped.
        #[cfg(windows)]
        let parent = unsafe { Window::from_win32_hwnd(window.handle()) };
        // SAFETY: as above.
        #[cfg(windows)]
        if let Err(error) = unsafe { gui.set_parent(&plugin, parent) } {
            gui.destroy(&plugin);
            return Err(failed("the plugin could not use the host's window")(error));
        }
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            // Use the raw, pinned CLAP ABI rather than guessing a convenience
            // constructor's argument type. x11 is an unsigned-long XID;
            // cocoa is an NSView pointer. Neither is an NSWindow pointer.
            let mut parent: clap_sys::ext::gui::clap_window = unsafe { std::mem::zeroed() };
            #[cfg(target_os = "macos")]
            {
                parent.api = c"cocoa".as_ptr();
                parent.specific.cocoa = window.handle();
            }
            #[cfg(target_os = "linux")]
            {
                parent.api = c"x11".as_ptr();
                parent.specific.x11 = window.handle() as usize as _;
            }
            let accepted = unsafe {
                (*raw_gui)
                    .set_parent
                    .is_some_and(|set| set(raw_plugin, &parent))
            };
            if !accepted {
                gui.destroy(&plugin);
                return Err(EditorError::Failed(
                    "the plugin refused the host parent".into(),
                ));
            }
        }
        #[cfg(windows)]
        let _ = gui.show(&plugin);
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        if let Err(error) = gui.show(&plugin) {
            gui.destroy(&plugin);
            return Err(failed("the plugin could not show its editor")(error));
        }
        window.show(options.take_focus);

        let info = EditorInfo {
            window: Some(window.handle() as usize),
            width: size.width,
            height: size.height,
            resizable,
        };
        Ok((
            Embedded {
                window,
                size: shared_size,
            },
            info,
        ))
    }
}
