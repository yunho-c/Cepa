use tauri::{App, Manager, Runtime, plugin::TauriPlugin};
use tauri_plugin_window_state::{AppHandleExt, Builder};

mod placement;
mod policy;

pub fn state_plugin<R: Runtime>() -> TauriPlugin<R> {
    Builder::default()
        .with_state_flags(policy::persisted_state_flags())
        .skip_initial_state("main")
        .build()
}

pub fn initialize<R: Runtime>(app: &mut App<R>) -> Result<(), Box<dyn std::error::Error>> {
    let window = app.get_webview_window("main").ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "the main window was not created",
        )
    })?;
    let state_path = app.path().app_config_dir()?.join(app.handle().filename());
    let restore_saved_state = placement::has_saved_state(&state_path);
    std::thread::spawn(move || {
        let placement = if restore_saved_state {
            placement::restore(&window, &state_path, policy::persisted_state_flags())
        } else {
            window.center()
        };
        if let Err(error) = placement {
            eprintln!("Could not place the main window: {error}");
        }
        if let Err(error) = window.show() {
            eprintln!("Could not show the main window: {error}");
        }
        let _ = window.set_focus();
    });
    Ok(())
}
