mod scanner;

use scanner::{DirectoryView, ScanProgress, ScanSnapshot};
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;
use tauri::ipc::Channel;

pub use scanner::{ScanBackend, ScanResult};

/// A completed benchmark scan. Retaining the snapshot keeps benchmark timing
/// aligned with the application, which stores it for interactive drill-down.
pub struct BenchmarkScan {
    pub result: ScanResult,
    pub initial_view_ms: f64,
    _snapshot: ScanSnapshot,
    _initial_view: DirectoryView,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CancellationMeasurement {
    pub entries_at_request: u64,
    pub scan_elapsed_us: u64,
    pub cancellation_latency_us: u64,
}

/// Runs the same portable scan and snapshot construction used by the desktop
/// application while retaining the completed snapshot through measurement.
pub fn benchmark_scan(path: &Path) -> Result<BenchmarkScan, String> {
    benchmark_scan_with_backend(path, ScanBackend::Jwalk)
}

/// Runs a benchmark scan with an explicitly selected traversal backend.
pub fn benchmark_scan_with_backend(
    path: &Path,
    backend: ScanBackend,
) -> Result<BenchmarkScan, String> {
    let output =
        scanner::scan_path_with_backend(path, Arc::new(AtomicBool::new(false)), backend, |_| {})?;
    let view_started_at = Instant::now();
    let initial_view = output.snapshot.directory_view(0, 0)?;
    Ok(BenchmarkScan {
        result: output.result,
        initial_view_ms: view_started_at.elapsed().as_secs_f64() * 1_000.0,
        _snapshot: output.snapshot,
        _initial_view: initial_view,
    })
}

/// Measures how long a scan takes to return after cancellation is requested
/// from a separate thread at a progress boundary.
pub fn benchmark_cancellation(
    path: &Path,
    backend: ScanBackend,
    cancel_after_entries: u64,
) -> Result<CancellationMeasurement, String> {
    if cancel_after_entries == 0 {
        return Err("cancel-after entries must be greater than zero".to_string());
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_from_thread = cancel.clone();
    let (trigger_sender, trigger_receiver) = mpsc::sync_channel::<u64>(1);
    let canceller = thread::spawn(move || {
        trigger_receiver.recv().ok().map(|entries| {
            let requested_at = Instant::now();
            cancel_from_thread.store(true, Ordering::Relaxed);
            (entries, requested_at)
        })
    });

    let scan_started_at = Instant::now();
    let scan_result = scanner::scan_path_with_backend(path, cancel, backend, |progress| {
        if progress.entries_scanned >= cancel_after_entries {
            let _ = trigger_sender.try_send(progress.entries_scanned);
        }
    });
    let scan_finished_at = Instant::now();
    drop(trigger_sender);

    let request = canceller
        .join()
        .map_err(|_| "the cancellation benchmark thread panicked".to_string())?;
    let Some((entries_at_request, requested_at)) = request else {
        return match scan_result {
            Ok(_) => Err(format!(
                "scan completed before reaching {cancel_after_entries} entries"
            )),
            Err(error) => Err(error),
        };
    };

    match scan_result {
        Err(error) if error == "Scan cancelled." => {}
        Err(error) => return Err(error),
        Ok(_) => {
            return Err(
                "scan completed before the asynchronous cancellation was observed".to_string(),
            );
        }
    }

    let cancellation_latency = scan_finished_at
        .checked_duration_since(requested_at)
        .ok_or_else(|| "cancellation was requested after the scan returned".to_string())?;
    Ok(CancellationMeasurement {
        entries_at_request,
        scan_elapsed_us: saturating_duration_us(scan_finished_at.duration_since(scan_started_at)),
        cancellation_latency_us: saturating_duration_us(cancellation_latency),
    })
}

fn saturating_duration_us(duration: std::time::Duration) -> u64 {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
}

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
        let view = snapshot.directory_view(id, 0);
        self.finish(id);
        let view = view?;
        *self
            .completed
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(CompletedScan { id, snapshot });
        Ok(view)
    }

    fn directory_view(&self, id: u64, node_id: u64) -> Result<DirectoryView, String> {
        let completed = self
            .completed
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let scan = completed
            .as_ref()
            .filter(|scan| scan.id == id)
            .ok_or_else(|| "That scan is no longer available.".to_string())?;
        scan.snapshot.directory_view(id, node_id)
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
    node_id: u64,
    state: tauri::State<'_, ScanState>,
) -> Result<DirectoryView, String> {
    state.directory_view(scan_id, node_id)
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
    use super::{ScanBackend, ScanState, benchmark_cancellation};
    use std::fs;
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

    #[test]
    fn measures_asynchronous_cancellation() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        for index in 0..2_500 {
            fs::write(temp.path().join(format!("file-{index}")), []).expect("write fixture file");
        }

        let measurement = benchmark_cancellation(temp.path(), ScanBackend::Jwalk, 2_048)
            .expect("measure cancellation");

        assert!(measurement.entries_at_request >= 2_048);
        assert!(measurement.scan_elapsed_us >= measurement.cancellation_latency_us);
    }
}
