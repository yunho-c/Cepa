use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

#[path = "../src/desktop_window/placement.rs"]
mod window_state_placement;
#[path = "../src/desktop_window/policy.rs"]
mod window_state_policy;

const STATE_FILENAME: &str = ".window-state-smoke.json";
const TARGET_OFFSET_X: i32 = 120;
const TARGET_OFFSET_Y: i32 = 140;
const TARGET_WIDTH: u32 = 900;
const TARGET_HEIGHT: u32 = 650;
const TOLERANCE: i64 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Mode {
    Seed,
    Verify,
}

#[derive(Clone, Copy, Debug, Deserialize)]
struct SavedWindowState {
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    maximized: bool,
}

fn main() {
    let mode = match std::env::args().nth(1).as_deref() {
        Some("seed") => Mode::Seed,
        Some("verify") => Mode::Verify,
        _ => {
            eprintln!("usage: window_state_smoke <seed|verify>");
            std::process::exit(2);
        }
    };
    let succeeded = Arc::new(AtomicBool::new(false));
    let state_path = Arc::new(Mutex::new(None));

    let app = tauri::Builder::default()
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_filename(STATE_FILENAME)
                .with_state_flags(window_state_policy::persisted_state_flags())
                .skip_initial_state("main")
                .build(),
        )
        .setup({
            let succeeded = Arc::clone(&succeeded);
            let state_path = Arc::clone(&state_path);
            move |app| {
                let window = app.get_webview_window("main").ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        "the main window was not created",
                    )
                })?;
                let path = app.path().app_config_dir()?.join(STATE_FILENAME);
                *state_path.lock().expect("lock state path") = Some(path.clone());
                let expected = match mode {
                    Mode::Seed => {
                        let _ = std::fs::remove_file(&path);
                        None
                    }
                    Mode::Verify => {
                        if !window_state_placement::has_saved_state(&path) {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                "the smoke state has no valid main-window geometry",
                            )
                            .into());
                        }
                        Some(read_saved_state(&path)?)
                    }
                };
                let app_handle = app.handle().clone();
                std::thread::spawn(move || {
                    if let Err(error) = begin_smoke(
                        mode,
                        window,
                        path,
                        expected,
                        Arc::clone(&succeeded),
                        app_handle.clone(),
                    ) {
                        eprintln!("could not initialize window-state smoke test: {error}");
                        app_handle.exit(1);
                    }
                });
                Ok(())
            }
        })
        .build(tauri::generate_context!())
        .expect("build window-state smoke application");

    let exit_code = app.run_return(|_, _| {});

    let path = state_path.lock().expect("lock state path").clone();
    if mode == Mode::Verify
        && let Some(path) = path
    {
        let _ = std::fs::remove_file(path);
    }
    if exit_code != 0 || !succeeded.load(Ordering::Acquire) {
        std::process::exit(1);
    }
}

fn begin_smoke(
    mode: Mode,
    window: WebviewWindow,
    state_path: std::path::PathBuf,
    expected: Option<SavedWindowState>,
    succeeded: Arc<AtomicBool>,
    app: AppHandle,
) -> tauri::Result<()> {
    match mode {
        Mode::Seed => {
            window.show()?;
            let monitor = window
                .current_monitor()?
                .or(window.primary_monitor()?)
                .ok_or_else(|| tauri::Error::AssetNotFound("no monitor is available".into()))?;
            let monitor_position = monitor.position();
            let target_x = monitor_position.x + TARGET_OFFSET_X;
            let target_y = monitor_position.y + TARGET_OFFSET_Y;
            window.set_position(PhysicalPosition::new(target_x, target_y))?;
            window.set_size(PhysicalSize::new(TARGET_WIDTH, TARGET_HEIGHT))?;
            std::thread::sleep(Duration::from_millis(500));
            let position = window.outer_position()?;
            let size = window.inner_size()?;
            let matches = close(position.x, target_x)
                && close(position.y, target_y)
                && close(size.width, TARGET_WIDTH)
                && close(size.height, TARGET_HEIGHT);
            succeeded.store(matches, Ordering::Release);
            app.exit(i32::from(!matches));
        }
        Mode::Verify => {
            let expected = expected.expect("verify mode requires saved state");
            window.show()?;
            window_state_placement::restore(
                &window,
                &state_path,
                window_state_policy::persisted_state_flags(),
            )?;
            std::thread::sleep(Duration::from_millis(500));
            let actual_position = window.outer_position();
            let actual_size = window.inner_size();
            let matches = match (actual_position, actual_size) {
                (Ok(position), Ok(size)) => {
                    let matches = close(position.x, expected.x)
                        && close(position.y, expected.y)
                        && close(size.width, expected.width)
                        && close(size.height, expected.height)
                        && !expected.maximized;
                    eprintln!(
                        "saved={}x{}@{},{} restored={}x{}@{},{}",
                        expected.width,
                        expected.height,
                        expected.x,
                        expected.y,
                        size.width,
                        size.height,
                        position.x,
                        position.y
                    );
                    matches
                }
                (Err(error), _) | (_, Err(error)) => {
                    eprintln!("could not inspect the restored window: {error}");
                    false
                }
            };
            succeeded.store(matches, Ordering::Release);
            app.exit(i32::from(!matches));
        }
    }
    Ok(())
}

fn read_saved_state(path: &std::path::Path) -> Result<SavedWindowState, std::io::Error> {
    let bytes = std::fs::read(path)?;
    let states: HashMap<String, SavedWindowState> = serde_json::from_slice(&bytes)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    states.get("main").cloned().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the saved state has no main window",
        )
    })
}

fn close(left: impl Into<i64>, right: impl Into<i64>) -> bool {
    (left.into() - right.into()).abs() <= TOLERANCE
}
