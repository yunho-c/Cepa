use std::path::Path;
#[cfg(target_os = "macos")]
use tauri::PhysicalPosition;
use tauri::{Runtime, WebviewWindow};
use tauri_plugin_window_state::{StateFlags, WindowExt};

pub(crate) fn restore<R: Runtime>(
    window: &WebviewWindow<R>,
    state_path: &Path,
    flags: StateFlags,
) -> tauri::Result<()> {
    window.restore_state(flags)?;

    #[cfg(not(target_os = "macos"))]
    let _ = state_path;

    // The plugin's macOS startup monitor query can be empty, which makes its
    // otherwise useful off-screen guard skip position restoration. Reapply the
    // same persisted position after its size/maximize work. Retain the off-screen
    // guard with the current/primary monitor as a safe fallback.
    #[cfg(target_os = "macos")]
    if flags.contains(StateFlags::POSITION)
        && let Some(saved) = read_saved_geometry(state_path)
    {
        let mut monitors = window.available_monitors().unwrap_or_default();
        if monitors.is_empty() {
            let fallback = window
                .current_monitor()
                .ok()
                .flatten()
                .or_else(|| window.primary_monitor().ok().flatten());
            if let Some(monitor) = fallback {
                monitors.push(monitor);
            }
        }
        if monitors.iter().any(|monitor| {
            intersects(
                saved,
                MonitorBounds {
                    x: monitor.position().x,
                    y: monitor.position().y,
                    width: monitor.size().width,
                    height: monitor.size().height,
                },
            )
        }) {
            window.set_position(PhysicalPosition::new(saved.x, saved.y))?;
        } else {
            window.center()?;
        }
    }

    Ok(())
}

pub(crate) fn has_saved_state(path: &Path) -> bool {
    read_saved_geometry(path).is_some_and(SavedGeometry::is_well_formed)
}

#[derive(Clone, Copy, Debug, serde::Deserialize)]
struct SavedGeometry {
    width: u32,
    height: u32,
    x: i32,
    y: i32,
}

impl SavedGeometry {
    fn is_well_formed(self) -> bool {
        i64::from(self.x) + i64::from(self.width) > i64::from(self.x)
            && i64::from(self.y) + i64::from(self.height) > i64::from(self.y)
    }
}

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, Debug)]
struct MonitorBounds {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn read_saved_geometry(path: &Path) -> Option<SavedGeometry> {
    parse_saved_geometry(&std::fs::read(path).ok()?)
}

fn parse_saved_geometry(bytes: &[u8]) -> Option<SavedGeometry> {
    let states: std::collections::HashMap<String, SavedGeometry> =
        serde_json::from_slice(bytes).ok()?;
    states.get("main").copied()
}

#[cfg(any(target_os = "macos", test))]
fn intersects(window: SavedGeometry, monitor: MonitorBounds) -> bool {
    let left = i64::from(monitor.x);
    let right = left + i64::from(monitor.width);
    let top = i64::from(monitor.y);
    let bottom = top + i64::from(monitor.height);
    let window_left = i64::from(window.x);
    let window_right = window_left + i64::from(window.width);
    let window_top = i64::from(window.y);
    let window_bottom = window_top + i64::from(window.height);

    [
        (window_left, window_top),
        (window_right, window_top),
        (window_left, window_bottom),
        (window_right, window_bottom),
    ]
    .into_iter()
    .any(|(x, y)| x >= left && x < right && y >= top && y < bottom)
}

#[cfg(test)]
mod tests {
    use super::{MonitorBounds, SavedGeometry, intersects, parse_saved_geometry};

    const MONITOR: MonitorBounds = MonitorBounds {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };

    #[test]
    fn accepts_partially_visible_windows_and_rejects_offscreen_state() {
        assert!(intersects(
            SavedGeometry {
                x: -100,
                y: 50,
                width: 900,
                height: 650,
            },
            MONITOR,
        ));
        assert!(!intersects(
            SavedGeometry {
                x: 4_000,
                y: 2_000,
                width: 900,
                height: 650,
            },
            MONITOR,
        ));
    }

    #[test]
    fn corrupted_extreme_geometry_cannot_overflow_visibility_checks() {
        assert!(!intersects(
            SavedGeometry {
                x: i32::MAX,
                y: i32::MAX,
                width: u32::MAX,
                height: u32::MAX,
            },
            MonitorBounds {
                x: i32::MIN,
                y: i32::MIN,
                width: u32::MAX,
                height: u32::MAX,
            },
        ));
    }

    #[test]
    fn saved_state_requires_complete_main_window_geometry() {
        let state = parse_saved_geometry(
            br#"{"main":{"width":900,"height":650,"x":120,"y":140,"visible":false}}"#,
        )
        .expect("parse complete main geometry");
        assert_eq!(state.width, 900);
        assert_eq!(state.height, 650);
        assert_eq!(state.x, 120);
        assert_eq!(state.y, 140);

        assert!(parse_saved_geometry(br#"{"main":{"width":900}}"#).is_none());
        assert!(parse_saved_geometry(b"not json").is_none());
        assert!(
            !SavedGeometry {
                width: 0,
                height: 650,
                x: 120,
                y: 140,
            }
            .is_well_formed()
        );
    }
}
