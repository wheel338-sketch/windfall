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
    #[cfg(windows)]
    window: Option<embedded::Embedded>,
    info: EditorInfo,
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

        #[cfg(windows)]
        if can_embed {
            let (window, info) = embedded::open(instance, gui, embedded, options)?;
            instance.access_shared_handler(|shared| {
                shared.editor_open.store(true, Ordering::Release);
            });
            return Ok(Self {
                gui,
                window: Some(window),
                info,
            });
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
            #[cfg(windows)]
            window: None,
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

    pub fn set_scale(&mut self, instance: &mut PluginInstance<WindfallHost>, application: f64) -> Result<EditorInfo, EditorError> {
        #[cfg(windows)]
        {
            let window = self.window.as_ref().ok_or(EditorError::UnsupportedScale)?;
            let factor = crate::gui::scaling::content_scale(application, crate::gui::win32::window_scale(window.window.handle()))?;
            let plugin = instance.plugin_handle();
            self.gui.set_scale(&plugin, factor).map_err(|_| EditorError::UnsupportedScale)?;
            let size = self.gui.get_size(&plugin).map_err(failed("scaled editor did not supply a size"))?;
            if !(1..=32768).contains(&size.width) || !(1..=32768).contains(&size.height) {
                return Err(EditorError::Failed("invalid scaled editor dimensions".into()));
            }
            self.apply_requested_size(size.width, size.height);
            Ok(self.info)
        }
        #[cfg(not(windows))]
        {
            let _ = (instance, application);
            Err(EditorError::UnsupportedScale)
        }
    }

    /// True once after the user asked to close the host's window.
    pub fn take_close_request(&self) -> bool {
        #[cfg(windows)]
        if let Some(window) = &self.window {
            return window.window.take_close_request();
        }
        false
    }

    /// Gives the editor the size the plugin asked for. Returns the new size
    /// if it changed.
    pub fn apply_requested_size(&mut self, width: u32, height: u32) -> Option<(u32, u32)> {
        #[cfg(windows)]
        if let Some(window) = &self.window {
            window.window.set_client_size(width, height);
            window.size.set((width, height));
        }
        self.note_size(width, height)
    }

    /// Picks up a size the user dragged the window to. Returns the new size
    /// if it changed.
    pub fn poll_size(&mut self) -> Option<(u32, u32)> {
        #[cfg(windows)]
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
        let _ = self.gui.hide(&plugin);
        self.gui.destroy(&plugin);
    }
}

#[cfg(windows)]
mod embedded {
    //! The editor inside a host window. The window procedure has to ask
    //! the plugin about sizes while the user drags the frame, at a moment
    //! when no reference to the instance can be had. It calls the plugin's
    //! `gui` functions through raw pointers instead, and that is the
    //! `unsafe` in this module.

    use std::cell::Cell;
    use std::rc::Rc;

    use clack_extensions::gui::{GuiConfiguration, GuiSize, PluginGui, Window};
    use clack_host::prelude::*;
    use clap_sys::ext::gui::{CLAP_EXT_GUI, clap_plugin_gui};
    use clap_sys::plugin::clap_plugin;

    use super::super::handlers::WindfallHost;
    use super::failed;
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
                    set_size(self.plugin, width, height);
                }
            }
            self.size.set((width, height));
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
        let _ = gui.set_scale(&plugin, system_scale());
        let size = gui.get_size(&plugin).unwrap_or(FALLBACK_SIZE);
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

        // SAFETY: the window outlives the plugin's editor: `Editor::close`
        // destroys the plugin's part before the window is dropped.
        let parent = unsafe { Window::from_win32_hwnd(window.handle()) };
        // SAFETY: as above.
        if let Err(error) = unsafe { gui.set_parent(&plugin, parent) } {
            gui.destroy(&plugin);
            return Err(failed("the plugin could not use the host's window")(error));
        }
        // Some plugins report failure here and show themselves anyway.
        let _ = gui.show(&plugin);
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
