//! The Windfall desktop shell. It owns the project document and the audio
//! engine, and exposes them to the UI through the IPC calls listed in
//! docs/ARCHITECTURE.md.
//!
//! [`session`] is the whole of that with no window attached, which is how
//! the tests drive it. `commands` and `shell` are the thin layer that puts
//! it inside a Tauri app.

pub mod browser;
mod commands;
pub mod events;
mod files_commands;
pub mod library;
pub mod paths;
pub mod plugin_database;
pub mod plugins;
mod process_memory;
mod panel_layout;
mod updates;
pub mod samples;
pub mod session;
pub mod settings;
mod shell;
mod sync;
#[cfg(all(test, windows))]
mod test_alloc;
#[cfg(all(test, windows))]
#[global_allocator]
static TEST_ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;
pub mod template;

use tauri::{Manager, WindowEvent};

use session::Session;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .setup(|app| {
            app.manage(updates::Updates::default());
            app.manage(panel_layout::PanelLayouts::load(app.path().app_config_dir()?.join("panel-layouts.json")));
            let session = shell::start(app.handle())?;
            app.manage(session);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let Some(view) = window.app_handle().get_webview_window(window.label()) {
                match event {
                    WindowEvent::Moved(_) | WindowEvent::Resized(_) => panel_layout::capture(&view, false),
                    WindowEvent::CloseRequested { .. } => panel_layout::capture(&view, true),
                    _ => {}
                }
            }
            if matches!(event, WindowEvent::Destroyed)
                && let Some(session) = window.try_state::<Session>()
            {
                session.unsubscribe_realtime(window.label());
                session.analyzer_window_closed(window.label());
            }
        })
        .invoke_handler(commands::handler())
        .run(tauri::generate_context!())
        .expect("error while running the Windfall shell");
}
