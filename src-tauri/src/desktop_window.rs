use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tauri::webview::{PageLoadEvent, PageLoadPayload};
use tauri::{App, Manager, Runtime, Webview, plugin::TauriPlugin};
use tauri_plugin_window_state::{AppHandleExt, Builder};

#[path = "desktop_window/placement.rs"]
pub(crate) mod placement;
#[path = "desktop_window/policy.rs"]
pub(crate) mod policy;

pub(crate) const INITIAL_PAGE_LOAD_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Default)]
pub struct StartupWindowState(Arc<StartupSignal>);

#[derive(Default)]
struct StartupSignal {
    page_loaded: Mutex<bool>,
    page_loaded_changed: Condvar,
}

pub fn state_plugin<R: Runtime>() -> TauriPlugin<R> {
    state_plugin_with_filename(None)
}

pub fn state_plugin_with_filename<R: Runtime>(filename: Option<&str>) -> TauriPlugin<R> {
    let mut builder = Builder::default()
        .with_state_flags(policy::persisted_state_flags())
        .skip_initial_state("main");
    if let Some(filename) = filename {
        builder = builder.with_filename(filename);
    }
    builder.build()
}

pub fn initialize<R: Runtime>(app: &mut App<R>) -> Result<(), Box<dyn std::error::Error>> {
    let window = app.get_webview_window("main").ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "the main window was not created",
        )
    })?;
    configure_chrome(&window)?;
    let state_path = app.path().app_config_dir()?.join(app.handle().filename());
    let restore_saved_state = placement::has_saved_state(&state_path);
    let startup = app.state::<StartupWindowState>().inner().clone();
    std::thread::spawn(move || {
        let started_at = Instant::now();
        let placement = if restore_saved_state {
            placement::restore(&window, &state_path, policy::persisted_state_flags())
        } else {
            window.center()
        };
        if let Err(error) = placement {
            eprintln!("Could not place the main window: {error}");
        }
        let remaining = INITIAL_PAGE_LOAD_TIMEOUT.saturating_sub(started_at.elapsed());
        if !startup.wait_for_page_load(remaining) {
            eprintln!("The main webview did not finish loading before the startup timeout.");
        }
        if let Err(error) = window.show() {
            eprintln!("Could not show the main window: {error}");
        }
        let _ = window.set_focus();
    });
    Ok(())
}

/// macOS keeps its native traffic lights in the configured overlay title bar.
/// Other desktops use the webview controls, installed before the hidden window
/// is placed or shown. Decorations are deliberately not persisted window state.
pub fn configure_chrome<R: Runtime>(window: &tauri::WebviewWindow<R>) -> tauri::Result<()> {
    window.set_decorations(cfg!(target_os = "macos"))
}

pub fn handle_page_load<R: Runtime>(webview: &Webview<R>, payload: &PageLoadPayload<'_>) {
    if is_main_page_ready(webview.label(), payload.event()) {
        webview.state::<StartupWindowState>().mark_page_loaded();
    }
}

fn is_main_page_ready(label: &str, event: PageLoadEvent) -> bool {
    label == "main" && event == PageLoadEvent::Finished
}

impl StartupWindowState {
    fn mark_page_loaded(&self) {
        let mut page_loaded = self
            .0
            .page_loaded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *page_loaded = true;
        self.0.page_loaded_changed.notify_all();
    }

    pub(crate) fn wait_for_page_load(&self, timeout: Duration) -> bool {
        let page_loaded = self
            .0
            .page_loaded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (page_loaded, _) = self
            .0
            .page_loaded_changed
            .wait_timeout_while(page_loaded, timeout, |page_loaded| !*page_loaded)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *page_loaded
    }
}

#[cfg(test)]
mod startup_tests {
    use super::{PageLoadEvent, StartupWindowState, is_main_page_ready};
    use std::time::Duration;

    #[test]
    fn accepts_only_the_finished_main_webview() {
        assert!(is_main_page_ready("main", PageLoadEvent::Finished));
        assert!(!is_main_page_ready("main", PageLoadEvent::Started));
        assert!(!is_main_page_ready("secondary", PageLoadEvent::Finished));
    }

    #[test]
    fn remembers_page_completion_before_the_window_waits() {
        let startup = StartupWindowState::default();
        startup.mark_page_loaded();

        assert!(startup.wait_for_page_load(Duration::ZERO));
    }

    #[test]
    fn times_out_when_the_initial_page_never_finishes() {
        let startup = StartupWindowState::default();

        assert!(!startup.wait_for_page_load(Duration::ZERO));
    }
}
