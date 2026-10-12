//! Native detached geometry. No musical data or realtime work belongs here.
use crate::sync::lock;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};
use tauri::{Manager, WebviewWindow};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layout {
    monitor: Option<String>,
    /// Logical size and offset relative to the monitor.
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
#[derive(Debug, Clone)]
struct Screen {
    name: Option<String>,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    scale: f64,
}

fn recover(layout: &Layout, screens: &[Screen]) -> Option<(i32, i32, u32, u32)> {
    let screen = screens
        .iter()
        .find(|screen| screen.name == layout.monitor)
        .or(screens.first())?;
    if ![
        layout.x,
        layout.y,
        layout.width,
        layout.height,
        screen.scale,
    ]
    .iter()
    .all(|n| n.is_finite())
        || screen.scale <= 0.0
    {
        return None;
    }
    let width = (layout.width.clamp(320.0, 4096.0) * screen.scale)
        .round()
        .clamp(1.0, f64::from(screen.width.max(1))) as u32;
    let height = (layout.height.clamp(240.0, 4096.0) * screen.scale)
        .round()
        .clamp(1.0, f64::from(screen.height.saturating_sub(48).max(1))) as u32;
    let x = (f64::from(screen.x) + layout.x * screen.scale)
        .round()
        .clamp(
            f64::from(screen.x),
            f64::from(screen.x) + f64::from(screen.width.saturating_sub(width)),
        ) as i32;
    let y = (f64::from(screen.y) + layout.y * screen.scale)
        .round()
        .clamp(
            f64::from(screen.y),
            f64::from(screen.y) + f64::from(screen.height.saturating_sub(height + 48)),
        ) as i32;
    Some((x, y, width, height))
}

pub struct PanelLayouts {
    file: PathBuf,
    layouts: Mutex<BTreeMap<String, Layout>>,
}
impl PanelLayouts {
    pub fn load(file: PathBuf) -> Self {
        let layouts = std::fs::read(&file)
            .ok()
            .filter(|bytes| bytes.len() <= 16_384)
            .and_then(|bytes| serde_json::from_slice::<BTreeMap<String, Layout>>(&bytes).ok())
            .unwrap_or_default();
        Self {
            file,
            layouts: Mutex::new(layouts),
        }
    }
    pub fn capture(&self, window: &WebviewWindow) {
        if !window.label().starts_with("panel-") {
            return;
        }
        let (Ok(position), Ok(size), Ok(Some(monitor))) = (
            window.outer_position(),
            window.outer_size(),
            window.current_monitor(),
        ) else {
            return;
        };
        let scale = monitor.scale_factor();
        if !scale.is_finite() || scale <= 0.0 {
            return;
        }
        lock(&self.layouts).insert(
            window.label().into(),
            Layout {
                monitor: monitor.name().cloned(),
                x: (f64::from(position.x) - f64::from(monitor.position().x)) / scale,
                y: (f64::from(position.y) - f64::from(monitor.position().y)) / scale,
                width: f64::from(size.width) / scale,
                height: f64::from(size.height) / scale,
            },
        );
    }
    pub fn save(&self) {
        let Ok(bytes) = serde_json::to_vec(&*lock(&self.layouts)) else {
            return;
        };
        if let Some(parent) = self.file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = windfall_project::file::write_atomic(&self.file, &bytes) {
            log::warn!("Could not save detached layouts: {error}");
        }
    }
    pub fn restore(&self, window: &WebviewWindow) {
        let Some(layout) = lock(&self.layouts).get(window.label()).cloned() else {
            return;
        };
        let Ok(monitors) = window.available_monitors() else {
            return;
        };
        let screens = monitors
            .iter()
            .map(|m| Screen {
                name: m.name().cloned(),
                x: m.position().x,
                y: m.position().y,
                width: m.size().width,
                height: m.size().height,
                scale: m.scale_factor(),
            })
            .collect::<Vec<_>>();
        if let Some((x, y, width, height)) = recover(&layout, &screens) {
            let decoration = window
                .outer_size()
                .ok()
                .zip(window.inner_size().ok())
                .map(|(outer, inner)| {
                    (
                        outer.width.saturating_sub(inner.width),
                        outer.height.saturating_sub(inner.height),
                    )
                })
                .unwrap_or((0, 0));
            let _ = window.set_size(tauri::PhysicalSize::new(
                width.saturating_sub(decoration.0).max(1),
                height.saturating_sub(decoration.1).max(1),
            ));
            let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
        }
    }
}
pub fn restore(window: &WebviewWindow) {
    if let Some(layouts) = window.try_state::<PanelLayouts>() {
        layouts.restore(window);
    }
}
pub fn capture(window: &WebviewWindow, save: bool) {
    if let Some(layouts) = window.try_state::<PanelLayouts>() {
        layouts.capture(window);
        if save {
            layouts.save();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn screen(name: &str, x: i32, scale: f64) -> Screen {
        Screen {
            name: Some(name.into()),
            x,
            y: 0,
            width: 1920,
            height: 1080,
            scale,
        }
    }
    fn layout() -> Layout {
        Layout {
            monitor: Some("secondary".into()),
            x: 80.0,
            y: 40.0,
            width: 960.0,
            height: 640.0,
        }
    }
    #[test]
    fn layout_recovers_removed_monitor_and_negative_desktop_at_new_dpi() {
        assert_eq!(
            recover(&layout(), &[screen("primary", -1920, 1.0)]),
            Some((-1840, 40, 960, 640))
        );
        let (x, y, width, height) = recover(
            &layout(),
            &[screen("primary", 0, 1.0), screen("secondary", 1920, 2.0)],
        )
        .unwrap();
        assert!(x >= 1920 && x + width as i32 <= 3840);
        assert!(y >= 0 && y + height as i32 <= 1080);
    }
    #[test]
    fn layout_clamps_offscreen_and_corrupt_data() {
        let mut saved = layout();
        saved.x = -10_000.0;
        saved.y = 10_000.0;
        let (x, y, width, height) = recover(&saved, &[screen("secondary", 0, 1.0)]).unwrap();
        assert_eq!(x, 0);
        assert!(y + height as i32 <= 1080);
        assert_eq!(width, 960);
        saved.width = f64::NAN;
        assert!(recover(&saved, &[screen("secondary", 0, 1.0)]).is_none());
        assert!(recover(&layout(), &[]).is_none());
    }
    #[test]
    fn layout_file_roundtrip_and_corrupt_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("layouts.json");
        let store = PanelLayouts::load(file.clone());
        lock(&store.layouts).insert("panel-mixer".into(), layout());
        store.save();
        assert!(lock(&PanelLayouts::load(file.clone()).layouts).contains_key("panel-mixer"));
        std::fs::write(&file, b"broken").unwrap();
        assert!(lock(&PanelLayouts::load(file).layouts).is_empty());
    }
}
