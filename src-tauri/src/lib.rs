mod scanner;

use scanner::{DirectoryView, ScanProgress, ScanResult, ScanSnapshot};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;

#[derive(Default)]
struct ScanState {
    next_id: AtomicU64,
    active: Mutex<Option<ActiveScan>>,
    completed: Mutex<Option<CompletedScan>>,
}

struct ActiveScan {
    id: u64,
    cancel: Arc<AtomicBool>,
}

struct CompletedScan {
    id: u64,
    snapshot: ScanSnapshot,
}

impl ScanState {
    fn begin(&self) -> (u64, Arc<AtomicBool>) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let cancel = Arc::new(AtomicBool::new(false));
        *self
            .completed
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = None;
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());

        if let Some(previous) = active.replace(ActiveScan {
            id,
            cancel: cancel.clone(),
        }) {
            previous.cancel.store(true, Ordering::Relaxed);
        }

        (id, cancel)
    }

    fn cancel(&self, id: u64) -> bool {
        let active = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        active.as_ref().is_some_and(|scan| {
            if scan.id == id {
                scan.cancel.store(true, Ordering::Relaxed);
                true
            } else {
                false
            }
        })
    }

    fn finish(&self, id: u64) {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if active.as_ref().is_some_and(|scan| scan.id == id) {
            *active = None;
        }
    }

    fn complete(&self, id: u64, snapshot: ScanSnapshot) -> Result<DirectoryView, String> {
        let root = snapshot.root_path().to_string_lossy().into_owned();
        let view = snapshot.directory_view(id, &root);
        self.finish(id);
        let view = view?;
        *self
            .completed
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(CompletedScan { id, snapshot });
        Ok(view)
    }

    fn directory_view(&self, id: u64, path: &str) -> Result<DirectoryView, String> {
        let completed = self
            .completed
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let scan = completed
            .as_ref()
            .filter(|scan| scan.id == id)
            .ok_or_else(|| "That scan is no longer available.".to_string())?;
        scan.snapshot.directory_view(id, path)
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", rename_all = "camelCase")]
enum ScanEvent {
    Started {
        scan_id: u64,
        root: String,
    },
    Progress {
        scan_id: u64,
        progress: ScanProgress,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanResponse {
    scan_id: u64,
    result: ScanResult,
    view: DirectoryView,
}

#[tauri::command]
async fn scan_directory(
    path: String,
    on_event: Channel<ScanEvent>,
    state: tauri::State<'_, ScanState>,
) -> Result<ScanResponse, String> {
    let requested_path = PathBuf::from(path);
    let (scan_id, cancel) = state.begin();
    let _ = on_event.send(ScanEvent::Started {
        scan_id,
        root: requested_path.to_string_lossy().into_owned(),
    });

    let progress_channel = on_event.clone();
    let task = tauri::async_runtime::spawn_blocking(move || {
        scanner::scan_path(&requested_path, cancel, |progress| {
            let _ = progress_channel.send(ScanEvent::Progress { scan_id, progress });
        })
    });

    match task.await {
        Ok(Ok(output)) => {
            let view = state.complete(scan_id, output.snapshot)?;
            Ok(ScanResponse {
                scan_id,
                result: output.result,
                view,
            })
        }
        Ok(Err(error)) => {
            state.finish(scan_id);
            Err(error)
        }
        Err(error) => {
            state.finish(scan_id);
            Err(format!("The scanner stopped unexpectedly: {error}"))
        }
    }
}

#[tauri::command]
fn cancel_scan(scan_id: u64, state: tauri::State<'_, ScanState>) -> bool {
    state.cancel(scan_id)
}

#[tauri::command]
fn open_scan_directory(
    scan_id: u64,
    path: String,
    state: tauri::State<'_, ScanState>,
) -> Result<DirectoryView, String> {
    state.directory_view(scan_id, &path)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(ScanState::default())
        .invoke_handler(tauri::generate_handler![
            scan_directory,
            cancel_scan,
            open_scan_directory
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::ScanState;
    use std::sync::atomic::Ordering;

    #[test]
    fn starting_a_new_scan_cancels_the_previous_one() {
        let state = ScanState::default();
        let (_, first_cancel) = state.begin();
        let (second_id, second_cancel) = state.begin();

        assert!(first_cancel.load(Ordering::Relaxed));
        assert!(!second_cancel.load(Ordering::Relaxed));
        assert!(state.cancel(second_id));
        assert!(second_cancel.load(Ordering::Relaxed));
    }
}
