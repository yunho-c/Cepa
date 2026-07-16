#![cfg_attr(not(feature = "desktop"), allow(dead_code, unused_imports))]

mod compression;
#[cfg(feature = "desktop")]
mod desktop_menu;
#[cfg(feature = "desktop")]
mod desktop_window;
mod file_revision;
mod scan_roots;
mod scanner;

#[cfg(feature = "desktop")]
use scanner::{CompressionTarget, ScanProgress};
use scanner::{DirectoryView, ScanSnapshot, SizeMetric};
use serde::Serialize;
#[cfg(feature = "desktop")]
use std::collections::HashMap;
use std::path::Path;
#[cfg(feature = "desktop")]
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(feature = "desktop")]
use std::sync::Mutex;
#[cfg(feature = "desktop")]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
#[cfg(feature = "desktop")]
use std::time::Duration;
use std::time::Instant;
#[cfg(feature = "desktop")]
use tauri::Manager;
#[cfg(feature = "desktop")]
use tauri::ipc::Channel;
#[cfg(feature = "desktop")]
use tauri_plugin_opener::OpenerExt;

pub use scanner::{ScanBackend, ScanResult};

pub const AGGREGATION_CANCELLATION_CHECK_INTERVAL_NODES: usize =
    scanner::AGGREGATION_CANCELLATION_CHECK_INTERVAL_NODES;
pub const CONTENT_INTEGRITY_CHUNK_BYTES: usize = compression::CONTENT_HASH_CHUNK_BYTES;

/// A completed benchmark scan. Retaining the snapshot keeps benchmark timing
/// aligned with the application, which stores it for interactive drill-down.
pub struct BenchmarkScan {
    pub result: ScanResult,
    pub initial_view_ms: f64,
    snapshot: ScanSnapshot,
    initial_view: DirectoryView,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMeasurement {
    pub elapsed_us: u64,
    pub total_matches: usize,
    pub returned_items: usize,
    pub items_truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitialResponseMeasurement {
    pub response_bytes: usize,
    pub serialization_us: u64,
    pub list_items: usize,
    pub chart_items: usize,
}

impl BenchmarkScan {
    pub fn root_item_count(&self) -> usize {
        self.initial_view.total_items
    }

    /// Retained snapshot payload capacity, excluding allocator bookkeeping and
    /// the separately materialized initial response view.
    pub fn snapshot_retained_bytes(&self) -> usize {
        self.snapshot.retained_payload_bytes()
    }

    /// Measures only the synchronous destructor work for the retained snapshot.
    /// The result and initial wire view are released before timing begins.
    pub fn measure_snapshot_release_ms(self) -> f64 {
        let Self {
            result,
            snapshot,
            initial_view,
            ..
        } = self;
        drop(result);
        drop(initial_view);
        let started_at = Instant::now();
        drop(snapshot);
        started_at.elapsed().as_secs_f64() * 1_000.0
    }

    pub fn search_root(
        &self,
        query: &str,
        logical_size: bool,
    ) -> Result<SearchMeasurement, String> {
        let metric = if logical_size {
            SizeMetric::Logical
        } else {
            SizeMetric::Allocated
        };
        let started_at = Instant::now();
        let result =
            self.snapshot
                .search_directory(0, 0, metric, query, &AtomicBool::new(false))?;
        Ok(SearchMeasurement {
            elapsed_us: saturating_duration_us(started_at.elapsed()),
            total_matches: result.total_matches,
            returned_items: result.items.len(),
            items_truncated: result.items_truncated,
        })
    }

    pub fn measure_initial_response(&self) -> Result<InitialResponseMeasurement, String> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct BorrowedScanResponse<'a> {
            scan_id: u64,
            result: &'a ScanResult,
            view: &'a DirectoryView,
        }

        let response = BorrowedScanResponse {
            scan_id: 0,
            result: &self.result,
            view: &self.initial_view,
        };
        let started_at = Instant::now();
        let bytes = serde_json::to_vec(&response)
            .map_err(|error| format!("could not serialize the initial response: {error}"))?;

        Ok(InitialResponseMeasurement {
            response_bytes: bytes.len(),
            serialization_us: saturating_duration_us(started_at.elapsed()),
            list_items: self.initial_view.items.len(),
            chart_items: self
                .initial_view
                .chart_items
                .iter()
                .map(scanner::chart_item_count)
                .sum(),
        })
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CancellationMeasurement {
    pub entries_at_request: u64,
    pub scan_elapsed_us: u64,
    pub cancellation_latency_us: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AggregationCancellationMeasurement {
    pub nodes_at_request: usize,
    pub nodes_at_return: usize,
    pub foreground_elapsed_us: u64,
    pub cancellation_latency_us: u64,
    pub background_release_us: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentIntegrityMeasurement {
    pub logical_bytes: u64,
    pub bytes_read: u64,
    pub elapsed_us: u64,
    pub cancelled: bool,
    pub cancel_requested_at_bytes: Option<u64>,
    pub cancellation_latency_us: Option<u64>,
}

pub fn benchmark_content_integrity(
    path: &Path,
    cancel_after_bytes: Option<u64>,
) -> Result<ContentIntegrityMeasurement, String> {
    if let Some(cancel_after_bytes) = cancel_after_bytes {
        let logical_bytes = path
            .metadata()
            .map_err(|error| format!("could not read benchmark file metadata: {error}"))?
            .len();
        if cancel_after_bytes == 0
            || cancel_after_bytes
                > logical_bytes.saturating_sub(CONTENT_INTEGRITY_CHUNK_BYTES as u64)
        {
            return Err(
                "cancel-after bytes must leave at least one content-integrity chunk unread".into(),
            );
        }
        let path = path.to_path_buf();
        let cancel = Arc::new(AtomicBool::new(false));
        // Keep the worker at the requested chunk boundary until the controller
        // has published the cancellation flag.
        // Fast filesystems can otherwise finish the file before this thread is
        // scheduled, turning the cancellation measurement into a race.
        let (progress_tx, progress_rx) = mpsc::sync_channel(0);
        let (resume_tx, resume_rx) = mpsc::sync_channel(0);
        return thread::scope(|scope| {
            let worker_cancel = cancel.clone();
            let worker = scope.spawn(move || {
                compression::observe_content_integrity(&path, &worker_cancel, |offset| {
                    if offset >= cancel_after_bytes && progress_tx.send(offset).is_ok() {
                        let _ = resume_rx.recv();
                    }
                })
            });
            let requested_at_bytes = progress_rx.recv().map_err(|_| {
                "content-integrity worker stopped before the cancellation boundary".to_string()
            })?;
            let requested_at = Instant::now();
            cancel.store(true, Ordering::Release);
            resume_tx.send(()).map_err(|_| {
                "content-integrity worker stopped at the cancellation boundary".to_string()
            })?;
            let observation = worker
                .join()
                .map_err(|_| "content-integrity worker panicked".to_string())??;
            if !observation.cancelled {
                return Err("content-integrity work completed before cancellation".into());
            }
            Ok(ContentIntegrityMeasurement {
                logical_bytes: observation.logical_bytes,
                bytes_read: observation.bytes_read,
                elapsed_us: observation.elapsed_us,
                cancelled: true,
                cancel_requested_at_bytes: Some(requested_at_bytes),
                cancellation_latency_us: Some(saturating_duration_us(requested_at.elapsed())),
            })
        });
    }

    let cancel = AtomicBool::new(false);
    let observation = compression::observe_content_integrity(path, &cancel, |_| {})?;
    Ok(ContentIntegrityMeasurement {
        logical_bytes: observation.logical_bytes,
        bytes_read: observation.bytes_read,
        elapsed_us: observation.elapsed_us,
        cancelled: false,
        cancel_requested_at_bytes: None,
        cancellation_latency_us: None,
    })
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
        snapshot: output.snapshot,
        initial_view,
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
    let (acknowledge_sender, acknowledge_receiver) = mpsc::sync_channel::<()>(0);
    let canceller = thread::spawn(move || {
        trigger_receiver.recv().ok().map(|entries| {
            let requested_at = Instant::now();
            cancel_from_thread.store(true, Ordering::Relaxed);
            let _ = acknowledge_sender.send(());
            (entries, requested_at)
        })
    });

    let scan_started_at = Instant::now();
    let mut cancellation_requested = false;
    let scan_result = scanner::scan_path_with_backend(path, cancel, backend, |progress| {
        if !cancellation_requested && progress.entries_scanned >= cancel_after_entries {
            cancellation_requested = trigger_sender.try_send(progress.entries_scanned).is_ok();
            if cancellation_requested {
                let _ = acknowledge_receiver.recv();
            }
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

/// Measures cancellation requested from a separate thread while the production
/// bottom-up aggregation loop is processing a synthetic retained-node arena.
pub fn benchmark_aggregation_cancellation(
    node_count: usize,
    cancel_after_nodes: usize,
) -> Result<AggregationCancellationMeasurement, String> {
    if node_count <= AGGREGATION_CANCELLATION_CHECK_INTERVAL_NODES {
        return Err(format!(
            "node count must exceed the {}-node cancellation interval",
            AGGREGATION_CANCELLATION_CHECK_INTERVAL_NODES
        ));
    }
    if cancel_after_nodes == 0 || cancel_after_nodes >= node_count {
        return Err("cancel-after nodes must be between one and node-count minus one".to_string());
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_from_thread = cancel.clone();
    let (trigger_sender, trigger_receiver) = mpsc::sync_channel::<usize>(1);
    let (acknowledge_sender, acknowledge_receiver) = mpsc::sync_channel::<()>(0);
    let canceller = thread::spawn(move || {
        trigger_receiver.recv().ok().map(|nodes| {
            let requested_at = Instant::now();
            cancel_from_thread.store(true, Ordering::Relaxed);
            let _ = acknowledge_sender.send(());
            (nodes, requested_at)
        })
    });

    let mut aggregation_started_at = None;
    let mut nodes_at_return = 0;
    let mut cancellation_requested = false;
    let (released_sender, released_receiver) = mpsc::sync_channel::<Instant>(1);
    let aggregation_result = scanner::aggregate_synthetic_nodes(
        node_count,
        &cancel,
        || aggregation_started_at = Some(Instant::now()),
        |processed| {
            nodes_at_return = processed;
            if !cancellation_requested && processed >= cancel_after_nodes {
                cancellation_requested = trigger_sender.try_send(processed).is_ok();
                if cancellation_requested {
                    let _ = acknowledge_receiver.recv();
                }
            }
        },
        move || {
            let _ = released_sender.send(Instant::now());
        },
    );
    let aggregation_finished_at = Instant::now();
    drop(trigger_sender);

    let request = canceller
        .join()
        .map_err(|_| "the aggregation cancellation thread panicked".to_string())?;
    let Some((nodes_at_request, requested_at)) = request else {
        return match aggregation_result {
            Ok(()) => Err(format!(
                "aggregation completed before reaching {cancel_after_nodes} nodes"
            )),
            Err(error) => Err(error),
        };
    };

    match aggregation_result {
        Err(error) if error == "Scan cancelled." => {}
        Err(error) => return Err(error),
        Ok(()) => {
            return Err(
                "aggregation completed before asynchronous cancellation was observed".to_string(),
            );
        }
    }

    let aggregation_started_at =
        aggregation_started_at.ok_or_else(|| "aggregation did not report its start".to_string())?;
    let release_finished_at = released_receiver
        .recv()
        .map_err(|_| "the cancelled aggregation arena was not released".to_string())?;
    Ok(AggregationCancellationMeasurement {
        nodes_at_request,
        nodes_at_return,
        foreground_elapsed_us: saturating_duration_us(
            aggregation_finished_at.duration_since(aggregation_started_at),
        ),
        cancellation_latency_us: saturating_duration_us(
            aggregation_finished_at.duration_since(requested_at),
        ),
        background_release_us: saturating_duration_us(
            release_finished_at.duration_since(aggregation_finished_at),
        ),
    })
}

fn saturating_duration_us(duration: std::time::Duration) -> u64 {
    duration.as_micros().min(u128::from(u64::MAX)) as u64
}

#[cfg(feature = "desktop")]
#[derive(Default)]
struct ScanState {
    next_id: AtomicU64,
    lifecycle: Mutex<ScanLifecycle>,
}

#[cfg(feature = "desktop")]
#[derive(Default)]
struct ScanLifecycle {
    active: Option<ActiveScan>,
    completed: Option<CompletedScan>,
}

#[cfg(feature = "desktop")]
#[derive(Default)]
struct ScanRootState {
    display_names: Mutex<HashMap<PathBuf, String>>,
}

#[cfg(feature = "desktop")]
impl ScanRootState {
    fn replace(&self, roots: &[scan_roots::ScanRoot]) {
        let names = roots
            .iter()
            .map(|root| (root.path().to_path_buf(), root.name().to_string()))
            .collect();
        *self
            .display_names
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = names;
    }

    fn display_name(&self, path: &Path) -> Option<String> {
        self.display_names
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(path)
            .cloned()
    }
}

#[cfg(feature = "desktop")]
struct ActiveScan {
    id: u64,
    cancel: Arc<AtomicBool>,
}

#[cfg(feature = "desktop")]
struct CompletedScan {
    id: u64,
    snapshot: Arc<ScanSnapshot>,
}

#[cfg(feature = "desktop")]
#[derive(Debug)]
struct RejectedScanCompletion {
    message: String,
    snapshot: Arc<ScanSnapshot>,
}

#[cfg(feature = "desktop")]
impl RejectedScanCompletion {
    fn into_parts(self) -> (String, Arc<ScanSnapshot>) {
        (self.message, self.snapshot)
    }
}

#[cfg(feature = "desktop")]
#[derive(Default)]
struct EstimateState {
    requests: CancellableRequestState,
}

#[cfg(feature = "desktop")]
#[derive(Default)]
struct SearchState {
    requests: CancellableRequestState,
}

#[cfg(feature = "desktop")]
#[derive(Default)]
struct CancellableRequestState {
    lifecycle: Mutex<CancellableRequestLifecycle>,
}

#[cfg(feature = "desktop")]
#[derive(Default)]
struct CancellableRequestLifecycle {
    next_token: u64,
    active: Option<ActiveRequest>,
}

#[cfg(feature = "desktop")]
struct ActiveRequest {
    token: u64,
    request_id: u64,
    cancel: Arc<AtomicBool>,
}

#[cfg(feature = "desktop")]
#[derive(Default)]
struct CompressionPlanState {
    lifecycle: Mutex<CompressionPlanLifecycle>,
}

#[cfg(feature = "desktop")]
#[derive(Default)]
struct CompressionPlanLifecycle {
    next_id: u64,
    generation: u64,
    active: Option<ActiveCompressionPlan>,
    cancel: Option<Arc<AtomicBool>>,
}

#[cfg(feature = "desktop")]
struct ActiveCompressionPlan {
    generation: u64,
    plan: compression::PreparedCompressionPlan,
}

#[cfg(feature = "desktop")]
impl CompressionPlanState {
    fn begin(&self) -> (u64, u64, Arc<AtomicBool>) {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        lifecycle.next_id = lifecycle
            .next_id
            .checked_add(1)
            .expect("compression plan ID space exhausted");
        lifecycle.generation = lifecycle
            .generation
            .checked_add(1)
            .expect("compression plan generation space exhausted");
        lifecycle.active = None;
        if let Some(previous) = lifecycle.cancel.replace(cancel.clone()) {
            previous.store(true, Ordering::Release);
        }
        (lifecycle.next_id, lifecycle.generation, cancel)
    }

    fn finish(
        &self,
        generation: u64,
        plan: compression::PreparedCompressionPlan,
    ) -> Result<compression::CompressionPlanPreview, String> {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if lifecycle.generation != generation
            || lifecycle.active.is_some()
            || lifecycle.cancel.is_none()
        {
            return Err("That compression plan was superseded by a newer request.".into());
        }
        let preview = plan.preview().clone();
        lifecycle.active = Some(ActiveCompressionPlan { generation, plan });
        Ok(preview)
    }

    fn fail(&self, generation: u64) {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if lifecycle.generation == generation && lifecycle.active.is_none() {
            if let Some(cancel) = lifecycle.cancel.take() {
                cancel.store(true, Ordering::Release);
            }
            lifecycle.generation = lifecycle
                .generation
                .checked_add(1)
                .expect("compression plan generation space exhausted");
        }
    }

    fn get(
        &self,
        plan_id: u64,
    ) -> Result<(u64, compression::PreparedCompressionPlan, Arc<AtomicBool>), String> {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let active = lifecycle
            .active
            .as_ref()
            .filter(|active| active.plan.preview().plan_id == plan_id)
            .ok_or_else(|| "That compression plan is no longer available.".to_string())?;
        let cancel = lifecycle
            .cancel
            .as_ref()
            .cloned()
            .ok_or_else(|| "That compression plan is no longer available.".to_string())?;
        Ok((active.generation, active.plan.clone(), cancel))
    }

    fn is_current(&self, generation: u64, plan_id: u64) -> bool {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        lifecycle.generation == generation
            && lifecycle
                .active
                .as_ref()
                .is_some_and(|active| active.plan.preview().plan_id == plan_id)
    }

    fn clear(&self) {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        lifecycle.generation = lifecycle
            .generation
            .checked_add(1)
            .expect("compression plan generation space exhausted");
        lifecycle.active = None;
        if let Some(cancel) = lifecycle.cancel.take() {
            cancel.store(true, Ordering::Release);
        }
    }

    #[cfg(test)]
    fn generation(&self) -> u64 {
        self.lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .generation
    }
}

#[cfg(feature = "desktop")]
impl CancellableRequestState {
    fn begin(&self, request_id: u64) -> (u64, Arc<AtomicBool>) {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        lifecycle.next_token = lifecycle
            .next_token
            .checked_add(1)
            .expect("request token space exhausted");
        let token = lifecycle.next_token;
        if let Some(previous) = lifecycle.active.replace(ActiveRequest {
            token,
            request_id,
            cancel: cancel.clone(),
        }) {
            previous.cancel.store(true, Ordering::Relaxed);
        }
        (token, cancel)
    }

    fn cancel(&self, request_id: u64) -> bool {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        lifecycle.active.as_ref().is_some_and(|request| {
            if request.request_id == request_id {
                request.cancel.store(true, Ordering::Relaxed);
                true
            } else {
                false
            }
        })
    }

    fn cancel_active(&self) {
        if let Some(active) = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .active
            .take()
        {
            active.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn finish(&self, token: u64) {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if lifecycle
            .active
            .as_ref()
            .is_some_and(|request| request.token == token)
        {
            lifecycle.active = None;
        }
    }
}

#[cfg(feature = "desktop")]
impl EstimateState {
    fn begin(&self, request_id: u64) -> (u64, Arc<AtomicBool>) {
        self.requests.begin(request_id)
    }

    fn cancel(&self, request_id: u64) -> bool {
        self.requests.cancel(request_id)
    }

    fn cancel_active(&self) {
        self.requests.cancel_active();
    }

    fn finish(&self, token: u64) {
        self.requests.finish(token);
    }
}

#[cfg(feature = "desktop")]
impl SearchState {
    fn begin(&self, request_id: u64) -> (u64, Arc<AtomicBool>) {
        self.requests.begin(request_id)
    }

    fn cancel(&self, request_id: u64) -> bool {
        self.requests.cancel(request_id)
    }

    fn cancel_active(&self) {
        self.requests.cancel_active();
    }

    fn finish(&self, token: u64) {
        self.requests.finish(token);
    }
}

#[cfg(feature = "desktop")]
impl ScanState {
    fn begin(&self) -> (u64, Arc<AtomicBool>, Option<Arc<ScanSnapshot>>) {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        // Allocate IDs while holding the lifecycle lock so concurrent starts
        // cannot install a lower ID after a higher ID already owns the state.
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let released_snapshot = lifecycle
            .completed
            .take()
            .map(|completed| completed.snapshot);

        if let Some(previous) = lifecycle.active.replace(ActiveScan {
            id,
            cancel: cancel.clone(),
        }) {
            previous.cancel.store(true, Ordering::Relaxed);
        }

        (id, cancel, released_snapshot)
    }

    fn cancel(&self, id: u64) -> bool {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        lifecycle.active.as_ref().is_some_and(|scan| {
            if scan.id == id {
                scan.cancel.store(true, Ordering::Relaxed);
                true
            } else {
                false
            }
        })
    }

    fn finish(&self, id: u64) {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if lifecycle.active.as_ref().is_some_and(|scan| scan.id == id) {
            lifecycle.active = None;
        }
    }

    fn complete(
        &self,
        id: u64,
        snapshot: ScanSnapshot,
    ) -> Result<DirectoryView, RejectedScanCompletion> {
        let snapshot = Arc::new(snapshot);
        let view = snapshot.directory_view(id, 0);
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());

        if lifecycle.active.as_ref().is_none_or(|scan| scan.id != id) {
            return Err(RejectedScanCompletion {
                message: "That scan was superseded by a newer request.".to_string(),
                snapshot,
            });
        }
        lifecycle.active = None;

        let view = view.map_err(|message| RejectedScanCompletion {
            message,
            snapshot: Arc::clone(&snapshot),
        })?;
        lifecycle.completed = Some(CompletedScan { id, snapshot });
        Ok(view)
    }

    fn detach(&self, id: u64) -> Result<Option<Arc<ScanSnapshot>>, String> {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match lifecycle.completed.as_ref() {
            Some(scan) if scan.id == id => {
                let snapshot = lifecycle
                    .completed
                    .take()
                    .expect("the matching completed scan is present")
                    .snapshot;
                Ok(Some(snapshot))
            }
            Some(_) => Err("That scan is no longer available.".to_string()),
            None => Ok(None),
        }
    }

    fn snapshot(&self, id: u64) -> Result<Arc<ScanSnapshot>, String> {
        let lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        lifecycle
            .completed
            .as_ref()
            .filter(|scan| scan.id == id)
            .map(|scan| scan.snapshot.clone())
            .ok_or_else(|| "That scan is no longer available.".to_string())
    }

    fn directory_view(
        &self,
        id: u64,
        node_id: u64,
        metric: SizeMetric,
    ) -> Result<DirectoryView, String> {
        self.snapshot(id)?
            .directory_view_with_metric(id, node_id, metric)
    }

    fn reveal_path(&self, id: u64, node_id: u64) -> Result<PathBuf, String> {
        self.snapshot(id)?.reveal_path(node_id)
    }

    fn root_path(&self, id: u64) -> Result<PathBuf, String> {
        Ok(self.snapshot(id)?.root_path())
    }

    fn compression_target(&self, id: u64, node_id: u64) -> Result<CompressionTarget, String> {
        self.snapshot(id)?.compression_target(node_id)
    }
}

#[cfg(feature = "desktop")]
#[derive(Debug, Serialize)]
#[serde(
    tag = "event",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum ScanEvent {
    Started {
        scan_id: u64,
        root: String,
    },
    Progress {
        scan_id: u64,
        progress: ScanProgress,
    },
    Completed {
        response: Box<ScanResponse>,
    },
    Failed {
        scan_id: u64,
        message: String,
    },
}

#[cfg(feature = "desktop")]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanResponse {
    scan_id: u64,
    result: ScanResult,
    view: DirectoryView,
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn list_scan_roots(
    state: tauri::State<'_, ScanRootState>,
) -> Result<Vec<scan_roots::ScanRoot>, String> {
    let roots = tauri::async_runtime::spawn_blocking(scan_roots::discover_scan_roots)
        .await
        .map_err(|error| format!("Storage discovery stopped unexpectedly: {error}"))?;
    state.replace(&roots);
    Ok(roots)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn validate_scan_root(path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        scanner::validate_scan_root(Path::new(&path))
            .map(|(root, _)| root.to_string_lossy().into_owned())
    })
    .await
    .map_err(|error| format!("The folder check stopped unexpectedly: {error}"))?
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn scan_directory(
    path: String,
    on_event: Channel<ScanEvent>,
    app: tauri::AppHandle,
) -> Result<u64, String> {
    app.state::<EstimateState>().cancel_active();
    app.state::<SearchState>().cancel_active();
    app.state::<CompressionPlanState>().clear();
    let requested_path = PathBuf::from(path);
    let root_display_name = app.state::<ScanRootState>().display_name(&requested_path);
    let (scan_id, cancel, released_snapshot) = app.state::<ScanState>().begin();
    if let Some(snapshot) = released_snapshot {
        release_last_arc_on_blocking_pool(snapshot);
    }
    if let Err(error) = on_event.send(ScanEvent::Started {
        scan_id,
        root: requested_path.to_string_lossy().into_owned(),
    }) {
        app.state::<ScanState>().finish(scan_id);
        return Err(format!(
            "The scan progress channel could not start: {error}"
        ));
    }

    let progress_channel = on_event.clone();
    let scan_app = app.clone();
    // Keep the command response lane free so Stop can reach Rust while this
    // detached task owns traversal and the terminal channel event.
    drop(tauri::async_runtime::spawn(async move {
        let task = tauri::async_runtime::spawn_blocking(move || {
            let mut output = scanner::scan_path(&requested_path, cancel, |progress| {
                let _ = progress_channel.send(ScanEvent::Progress { scan_id, progress });
            })?;
            if let Some(display_name) = root_display_name {
                output.set_root_display_name(display_name);
            }
            Ok::<_, String>(output)
        });

        let scans = scan_app.state::<ScanState>();
        let event = match task.await {
            Ok(Ok(output)) => match scans.complete(scan_id, output.snapshot) {
                Ok(view) => ScanEvent::Completed {
                    response: Box::new(ScanResponse {
                        scan_id,
                        result: output.result,
                        view,
                    }),
                },
                Err(rejected) => {
                    let (message, snapshot) = rejected.into_parts();
                    release_last_arc_on_blocking_pool(snapshot);
                    ScanEvent::Failed { scan_id, message }
                }
            },
            Ok(Err(message)) => {
                scans.finish(scan_id);
                ScanEvent::Failed { scan_id, message }
            }
            Err(error) => {
                scans.finish(scan_id);
                ScanEvent::Failed {
                    scan_id,
                    message: format!("The scanner stopped unexpectedly: {error}"),
                }
            }
        };

        let completed = matches!(event, ScanEvent::Completed { .. });
        if on_event.send(event).is_err()
            && completed
            && let Ok(Some(snapshot)) = scans.detach(scan_id)
        {
            release_last_arc_on_blocking_pool(snapshot);
        }
    }));

    Ok(scan_id)
}

#[cfg(feature = "desktop")]
fn discard_scan_state(
    scan_id: u64,
    scans: &ScanState,
    estimates: &EstimateState,
    searches: &SearchState,
    plans: &CompressionPlanState,
) -> Result<Option<Arc<ScanSnapshot>>, String> {
    let snapshot = scans.detach(scan_id)?;
    estimates.cancel_active();
    searches.cancel_active();
    plans.clear();
    Ok(snapshot)
}

#[cfg(feature = "desktop")]
fn release_last_arc_on_blocking_pool<T>(value: Arc<T>)
where
    T: Send + Sync + 'static,
{
    drop(tauri::async_runtime::spawn_blocking(move || {
        let mut value = value;
        loop {
            match Arc::try_unwrap(value) {
                Ok(value) => {
                    drop(value);
                    break;
                }
                Err(shared) => {
                    value = shared;
                    thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }));
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn discard_scan(
    scan_id: u64,
    scans: tauri::State<'_, ScanState>,
    estimates: tauri::State<'_, EstimateState>,
    searches: tauri::State<'_, SearchState>,
    plans: tauri::State<'_, CompressionPlanState>,
) -> Result<(), String> {
    if let Some(snapshot) = discard_scan_state(scan_id, &scans, &estimates, &searches, &plans)? {
        release_last_arc_on_blocking_pool(snapshot);
    }
    Ok(())
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn cancel_scan(scan_id: u64, state: tauri::State<'_, ScanState>) -> bool {
    state.cancel(scan_id)
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn open_scan_directory(
    scan_id: u64,
    node_id: u64,
    metric: SizeMetric,
    state: tauri::State<'_, ScanState>,
) -> Result<DirectoryView, String> {
    state.directory_view(scan_id, node_id, metric)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn search_scan_directory(
    scan_id: u64,
    node_id: u64,
    metric: SizeMetric,
    query: String,
    request_id: u64,
    state: tauri::State<'_, ScanState>,
    searches: tauri::State<'_, SearchState>,
) -> Result<scanner::DirectorySearchResult, String> {
    let snapshot = state.snapshot(scan_id)?;
    let (token, cancel) = searches.begin(request_id);
    let task = tauri::async_runtime::spawn_blocking(move || {
        snapshot.search_directory(scan_id, node_id, metric, &query, &cancel)
    });
    let result = task
        .await
        .map_err(|error| format!("The folder search stopped unexpectedly: {error}"));
    searches.finish(token);
    result?
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn cancel_directory_search(request_id: u64, searches: tauri::State<'_, SearchState>) -> bool {
    searches.cancel(request_id)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn compression_capability(
    scan_id: u64,
    state: tauri::State<'_, ScanState>,
) -> Result<compression::CompressionCapability, String> {
    let root = state.root_path(scan_id)?;
    tauri::async_runtime::spawn_blocking(move || compression::probe(&root))
        .await
        .map_err(|error| format!("The compression capability task stopped unexpectedly: {error}"))
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn compression_state(
    scan_id: u64,
    node_id: u64,
    state: tauri::State<'_, ScanState>,
) -> Result<compression::CompressionState, String> {
    let target = state.compression_target(scan_id, node_id)?;
    tauri::async_runtime::spawn_blocking(move || compression::inspect(&target.path, target.kind))
        .await
        .map_err(|error| format!("The compression-state task stopped unexpectedly: {error}"))
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn estimate_compression_savings(
    scan_id: u64,
    node_id: u64,
    request_id: u64,
    scans: tauri::State<'_, ScanState>,
    estimates: tauri::State<'_, EstimateState>,
) -> Result<compression::SavingsEstimate, String> {
    let target = scans.compression_target(scan_id, node_id)?;
    let (token, cancel) = estimates.begin(request_id);
    let task =
        tauri::async_runtime::spawn_blocking(move || compression::estimate(&target, &cancel));
    let result = task
        .await
        .map_err(|error| format!("The savings-estimation task stopped unexpectedly: {error}"));
    estimates.finish(token);
    result
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn cancel_compression_estimate(
    request_id: u64,
    estimates: tauri::State<'_, EstimateState>,
) -> bool {
    estimates.cancel(request_id)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn prepare_compression_plan(
    scan_id: u64,
    node_id: u64,
    operation: compression::CompressionOperation,
    scans: tauri::State<'_, ScanState>,
    plans: tauri::State<'_, CompressionPlanState>,
) -> Result<compression::CompressionPlanPreview, String> {
    let target = scans.compression_target(scan_id, node_id)?;
    let (plan_id, generation, cancel) = plans.begin();
    let task = tauri::async_runtime::spawn_blocking(move || {
        compression::prepare_plan(plan_id, scan_id, node_id, target, operation, &cancel)
    });
    match task.await {
        Ok(Ok(plan)) => plans.finish(generation, plan),
        Ok(Err(error)) => {
            plans.fail(generation);
            Err(error)
        }
        Err(error) => {
            plans.fail(generation);
            Err(format!(
                "The compression-planning task stopped unexpectedly: {error}"
            ))
        }
    }
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn revalidate_compression_plan(
    plan_id: u64,
    plans: tauri::State<'_, CompressionPlanState>,
) -> Result<compression::PlanValidation, String> {
    let (generation, plan, cancel) = plans.get(plan_id)?;
    let validation =
        tauri::async_runtime::spawn_blocking(move || compression::revalidate_plan(&plan, &cancel))
            .await
            .map_err(|error| format!("The plan-validation task stopped unexpectedly: {error}"))?;
    if !plans.is_current(generation, plan_id) {
        return Err("That compression plan is no longer available.".into());
    }
    Ok(validation)
}

#[cfg(feature = "desktop")]
#[tauri::command]
async fn reveal_scan_item(
    scan_id: u64,
    node_id: u64,
    app: tauri::AppHandle,
    state: tauri::State<'_, ScanState>,
) -> Result<(), String> {
    let path = state.reveal_path(scan_id, node_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        app.opener()
            .reveal_item_in_dir(&path)
            .map_err(|error| format!("Could not reveal {}: {error}", path.display()))
    })
    .await
    .map_err(|error| format!("The file manager task stopped unexpectedly: {error}"))?
}

#[cfg(feature = "desktop")]
#[tauri::command]
fn set_desktop_menu_availability(
    availability: desktop_menu::DesktopMenuAvailability,
    app: tauri::AppHandle,
) -> Result<(), String> {
    desktop_menu::set_availability(&app, availability)
}

#[cfg(feature = "desktop")]
fn desktop_builder(
    window_state_plugin: tauri::plugin::TauriPlugin<tauri::Wry>,
) -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
        .menu(desktop_menu::build)
        .on_menu_event(|app, event| {
            desktop_menu::emit_command(app, event.id().as_ref());
        })
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(window_state_plugin)
        .manage(desktop_window::StartupWindowState::default())
        .on_page_load(desktop_window::handle_page_load)
        .setup(desktop_window::initialize)
        .manage(ScanState::default())
        .manage(ScanRootState::default())
        .manage(EstimateState::default())
        .manage(SearchState::default())
        .manage(CompressionPlanState::default())
        .invoke_handler(tauri::generate_handler![
            list_scan_roots,
            validate_scan_root,
            scan_directory,
            discard_scan,
            cancel_scan,
            open_scan_directory,
            search_scan_directory,
            cancel_directory_search,
            compression_capability,
            compression_state,
            estimate_compression_savings,
            cancel_compression_estimate,
            prepare_compression_plan,
            revalidate_compression_plan,
            reveal_scan_item,
            set_desktop_menu_availability
        ])
}

/// Builds the production desktop command and plugin graph with isolated window
/// state for the native scan smoke harness.
#[cfg(feature = "desktop")]
#[doc(hidden)]
pub fn desktop_builder_for_smoke(state_filename: &str) -> tauri::Builder<tauri::Wry> {
    desktop_builder(desktop_window::state_plugin_with_filename(Some(
        state_filename,
    )))
}

/// Applies the production startup placement and page-readiness behavior.
#[cfg(feature = "desktop")]
#[doc(hidden)]
pub fn initialize_desktop(
    app: &mut tauri::App<tauri::Wry>,
) -> Result<(), Box<dyn std::error::Error>> {
    desktop_window::initialize(app)
}

/// Waits for the production main WebView's initial page to finish loading.
#[cfg(feature = "desktop")]
#[doc(hidden)]
pub fn wait_for_desktop_page(app: &tauri::AppHandle<tauri::Wry>, timeout: Duration) -> bool {
    use tauri::Manager;

    app.state::<desktop_window::StartupWindowState>()
        .wait_for_page_load(timeout)
}

#[cfg(feature = "desktop")]
pub fn run() {
    desktop_builder(desktop_window::state_plugin())
        .setup(desktop_window::initialize)
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::{
        AGGREGATION_CANCELLATION_CHECK_INTERVAL_NODES, CONTENT_INTEGRITY_CHUNK_BYTES, ScanBackend,
        benchmark_aggregation_cancellation, benchmark_cancellation, benchmark_content_integrity,
    };
    #[cfg(feature = "desktop")]
    use super::{
        CompressionPlanState, EstimateState, ScanEvent, ScanState, SearchState, compression,
        discard_scan_state, scanner,
    };
    use std::fs;
    #[cfg(feature = "desktop")]
    use std::sync::Arc;
    #[cfg(feature = "desktop")]
    use std::sync::atomic::AtomicBool;
    #[cfg(feature = "desktop")]
    use std::sync::atomic::Ordering;

    #[cfg(feature = "desktop")]
    #[test]
    fn failed_scan_events_keep_the_terminal_channel_contract() {
        let event = serde_json::to_value(ScanEvent::Failed {
            scan_id: 17,
            message: "Scan cancelled.".to_string(),
        })
        .expect("serialize failed scan event");

        assert_eq!(event["event"], "failed");
        assert_eq!(event["scanId"], 17);
        assert_eq!(event["message"], "Scan cancelled.");
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn starting_a_new_scan_cancels_the_previous_one() {
        let state = ScanState::default();
        let (_, first_cancel, first_released) = state.begin();
        let (second_id, second_cancel, second_released) = state.begin();

        assert!(first_released.is_none());
        assert!(second_released.is_none());
        assert!(first_cancel.load(Ordering::Relaxed));
        assert!(!second_cancel.load(Ordering::Relaxed));
        assert!(state.cancel(second_id));
        assert!(second_cancel.load(Ordering::Relaxed));
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn concurrent_scan_starts_leave_the_highest_id_active() {
        const START_COUNT: usize = 16;

        let state = Arc::new(ScanState::default());
        let barrier = Arc::new(std::sync::Barrier::new(START_COUNT));
        let mut workers = Vec::with_capacity(START_COUNT);
        for _ in 0..START_COUNT {
            let state = Arc::clone(&state);
            let barrier = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                let (id, cancel, released) = state.begin();
                assert!(released.is_none());
                (id, cancel)
            }));
        }

        let mut starts = workers
            .into_iter()
            .map(|worker| worker.join().expect("scan starter must finish"))
            .collect::<Vec<_>>();
        starts.sort_unstable_by_key(|(id, _)| *id);
        assert_eq!(starts.len(), START_COUNT);
        assert_eq!(starts[0].0, 1);
        assert_eq!(starts[START_COUNT - 1].0, START_COUNT as u64);
        for (_, cancel) in &starts[..START_COUNT - 1] {
            assert!(cancel.load(Ordering::Relaxed));
        }

        let (active_id, active_cancel) = &starts[START_COUNT - 1];
        assert!(!active_cancel.load(Ordering::Relaxed));
        assert!(state.cancel(*active_id));
        assert!(active_cancel.load(Ordering::Relaxed));
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn starting_a_new_scan_detaches_the_previous_snapshot() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        fs::write(temp.path().join("payload.bin"), [1_u8]).expect("write fixture file");
        let output = scanner::scan_path(temp.path(), Arc::new(AtomicBool::new(false)), |_| {})
            .expect("scan fixture");
        let state = ScanState::default();
        let (scan_id, _, released) = state.begin();
        assert!(released.is_none());
        state
            .complete(scan_id, output.snapshot)
            .expect("retain snapshot");
        let retained = state.snapshot(scan_id).expect("clone retained snapshot");
        let retained_weak = Arc::downgrade(&retained);
        drop(retained);

        let (_, _, released) = state.begin();
        let released = released.expect("detach the previous snapshot");

        assert_eq!(
            state
                .root_path(scan_id)
                .expect_err("the prior snapshot must be unavailable"),
            "That scan is no longer available."
        );
        assert!(retained_weak.upgrade().is_some());
        drop(released);
        assert!(retained_weak.upgrade().is_none());
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn superseded_scan_cannot_install_its_snapshot() {
        let first_temp = tempfile::tempdir().expect("create first fixture directory");
        fs::write(first_temp.path().join("first.bin"), [1_u8]).expect("write first fixture file");
        let first_output =
            scanner::scan_path(first_temp.path(), Arc::new(AtomicBool::new(false)), |_| {})
                .expect("scan first fixture");

        let second_temp = tempfile::tempdir().expect("create second fixture directory");
        fs::write(second_temp.path().join("second.bin"), [2_u8])
            .expect("write second fixture file");
        let second_output =
            scanner::scan_path(second_temp.path(), Arc::new(AtomicBool::new(false)), |_| {})
                .expect("scan second fixture");

        let state = ScanState::default();
        let (first_id, first_cancel, _) = state.begin();
        let (second_id, second_cancel, _) = state.begin();
        assert!(first_cancel.load(Ordering::Relaxed));
        assert!(!second_cancel.load(Ordering::Relaxed));

        let rejected = state
            .complete(first_id, first_output.snapshot)
            .expect_err("the superseded scan must not install its snapshot");
        let (message, rejected_snapshot) = rejected.into_parts();
        assert_eq!(message, "That scan was superseded by a newer request.");
        let rejected_weak = Arc::downgrade(&rejected_snapshot);
        drop(rejected_snapshot);
        assert!(rejected_weak.upgrade().is_none());
        assert!(state.snapshot(first_id).is_err());
        assert!(state.snapshot(second_id).is_err());

        let view = state
            .complete(second_id, second_output.snapshot)
            .expect("the current scan must install its snapshot");
        assert_eq!(view.scan_id, second_id);
        assert_eq!(
            state
                .root_path(second_id)
                .expect("resolve current completed scan"),
            second_temp
                .path()
                .canonicalize()
                .expect("canonical second fixture root")
        );
        assert!(state.snapshot(first_id).is_err());
    }

    #[cfg(feature = "desktop")]
    fn assert_concurrent_request_ownership<State>(
        state: Arc<State>,
        begin: fn(&State, u64) -> (u64, Arc<AtomicBool>),
        cancel: fn(&State, u64) -> bool,
    ) where
        State: Send + Sync + 'static,
    {
        const START_COUNT: usize = 16;

        let barrier = Arc::new(std::sync::Barrier::new(START_COUNT));
        let mut workers = Vec::with_capacity(START_COUNT);
        for request_id in 100..100 + START_COUNT as u64 {
            let state = Arc::clone(&state);
            let barrier = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                let (token, cancel) = begin(&state, request_id);
                (token, request_id, cancel)
            }));
        }

        let mut starts = workers
            .into_iter()
            .map(|worker| worker.join().expect("request starter must finish"))
            .collect::<Vec<_>>();
        starts.sort_unstable_by_key(|(token, _, _)| *token);
        for (index, (token, _, cancellation)) in starts.iter().enumerate() {
            assert_eq!(*token, index as u64 + 1);
            assert_eq!(
                cancellation.load(Ordering::Relaxed),
                index + 1 < START_COUNT
            );
        }

        let (_, active_request_id, active_cancel) = &starts[START_COUNT - 1];
        assert!(!cancel(&state, starts[0].1));
        assert!(cancel(&state, *active_request_id));
        assert!(active_cancel.load(Ordering::Relaxed));
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn estimate_requests_cancel_superseded_work() {
        let state = EstimateState::default();
        let (_, first) = state.begin(10);
        let (second_token, second) = state.begin(11);

        assert!(first.load(Ordering::Relaxed));
        assert!(!second.load(Ordering::Relaxed));
        assert!(!state.cancel(10));
        assert!(state.cancel(11));
        assert!(second.load(Ordering::Relaxed));
        state.finish(second_token);
        assert!(!state.cancel(11));
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn finishing_an_old_estimate_cannot_clear_a_reused_wire_id() {
        let state = EstimateState::default();
        let (first_token, first) = state.begin(20);
        let (second_token, second) = state.begin(20);

        assert!(first.load(Ordering::Relaxed));
        state.finish(first_token);
        assert!(state.cancel(20));
        assert!(second.load(Ordering::Relaxed));
        state.finish(second_token);
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn search_requests_cancel_superseded_and_explicitly_cancelled_work() {
        let state = SearchState::default();
        let (_, first) = state.begin(30);
        let (second_token, second) = state.begin(31);

        assert!(first.load(Ordering::Relaxed));
        assert!(!second.load(Ordering::Relaxed));
        assert!(!state.cancel(30));
        assert!(state.cancel(31));
        assert!(second.load(Ordering::Relaxed));
        state.finish(second_token);
        assert!(!state.cancel(31));
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn concurrent_estimate_starts_leave_the_latest_token_active() {
        assert_concurrent_request_ownership(
            Arc::new(EstimateState::default()),
            EstimateState::begin,
            EstimateState::cancel,
        );
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn concurrent_search_starts_leave_the_latest_token_active() {
        assert_concurrent_request_ownership(
            Arc::new(SearchState::default()),
            SearchState::begin,
            SearchState::cancel,
        );
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn compression_plan_generations_reject_superseded_and_cleared_plans() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let path = temp.path().join("file.bin");
        fs::write(&path, vec![1_u8; 4096]).expect("write fixture file");
        let metadata = fs::metadata(&path).expect("read fixture metadata");
        #[cfg(unix)]
        let allocated_bytes = {
            use std::os::unix::fs::MetadataExt;
            metadata.blocks() * 512
        };
        #[cfg(not(unix))]
        let allocated_bytes = metadata.len();
        let scan_revision = crate::file_revision::snapshot_no_follow(&path)
            .ok()
            .map(|snapshot| snapshot.scanned);
        let target = scanner::CompressionTarget {
            path,
            kind: scanner::EntryKind::File,
            logical_bytes: metadata.len(),
            allocated_bytes,
            allocated_size_is_estimate: !cfg!(unix),
            scan_revision,
        };

        let state = CompressionPlanState::default();
        let (first_id, first_generation, first_cancel) = state.begin();
        let first = compression::prepare_plan(
            first_id,
            1,
            1,
            target.clone(),
            compression::CompressionOperation::Compress,
            &first_cancel,
        )
        .expect("prepare first plan");
        let (second_id, second_generation, second_cancel) = state.begin();
        assert!(
            first_cancel.load(Ordering::Acquire),
            "a newer plan must cancel the superseded preparation"
        );
        let second = compression::prepare_plan(
            second_id,
            1,
            1,
            target,
            compression::CompressionOperation::Compress,
            &second_cancel,
        )
        .expect("prepare second plan");

        assert!(state.finish(first_generation, first).is_err());
        assert_eq!(
            state
                .finish(second_generation, second)
                .expect("store current plan")
                .plan_id,
            second_id
        );
        assert!(state.get(first_id).is_err());
        let (_, in_flight, validation_cancel) = state.get(second_id).expect("clone current plan");
        let identity_anchor = in_flight.identity_anchor_weak();
        state.clear();
        assert!(state.get(second_id).is_err());
        assert!(
            validation_cancel.load(Ordering::Acquire),
            "clearing the plan must cancel in-flight content validation"
        );
        assert!(
            identity_anchor.upgrade().is_some(),
            "an in-flight validation clone must keep its anchor alive"
        );
        drop(in_flight);
        assert!(
            identity_anchor.upgrade().is_none(),
            "clearing the plan must release its anchor after in-flight work ends"
        );
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn failed_plan_preparation_invalidates_only_its_generation() {
        let state = CompressionPlanState::default();
        let (failed_id, failed_generation, failed_cancel) = state.begin();

        state.fail(failed_generation);
        assert!(failed_cancel.load(Ordering::Acquire));
        assert!(state.generation() > failed_generation);
        assert!(state.get(failed_id).is_err());

        let (_, current_generation, current_cancel) = state.begin();
        state.fail(failed_generation);
        assert_eq!(state.generation(), current_generation);
        assert!(!current_cancel.load(Ordering::Acquire));
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn concurrent_plan_starts_keep_generation_and_cancellation_together() {
        const START_COUNT: usize = 16;

        let state = Arc::new(CompressionPlanState::default());
        let barrier = Arc::new(std::sync::Barrier::new(START_COUNT));
        let mut workers = Vec::with_capacity(START_COUNT);
        for _ in 0..START_COUNT {
            let state = Arc::clone(&state);
            let barrier = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                barrier.wait();
                state.begin()
            }));
        }

        let mut starts = workers
            .into_iter()
            .map(|worker| worker.join().expect("plan starter must finish"))
            .collect::<Vec<_>>();
        starts.sort_unstable_by_key(|(_, generation, _)| *generation);
        for (index, (plan_id, generation, cancellation)) in starts.iter().enumerate() {
            let expected = index as u64 + 1;
            assert_eq!(*plan_id, expected);
            assert_eq!(*generation, expected);
            assert_eq!(
                cancellation.load(Ordering::Acquire),
                index + 1 < START_COUNT
            );
        }

        let (plan_id, generation, active_cancel) = &starts[START_COUNT - 1];
        let temp = tempfile::tempdir().expect("create fixture directory");
        let path = temp.path().join("file.bin");
        fs::write(&path, vec![1_u8; 4096]).expect("write fixture file");
        let metadata = fs::metadata(&path).expect("read fixture metadata");
        #[cfg(unix)]
        let allocated_bytes = {
            use std::os::unix::fs::MetadataExt;
            metadata.blocks() * 512
        };
        #[cfg(not(unix))]
        let allocated_bytes = metadata.len();
        let target = scanner::CompressionTarget {
            path: path.clone(),
            kind: scanner::EntryKind::File,
            logical_bytes: metadata.len(),
            allocated_bytes,
            allocated_size_is_estimate: !cfg!(unix),
            scan_revision: crate::file_revision::snapshot_no_follow(&path)
                .ok()
                .map(|snapshot| snapshot.scanned),
        };
        let plan = compression::prepare_plan(
            *plan_id,
            1,
            1,
            target,
            compression::CompressionOperation::Compress,
            active_cancel,
        )
        .expect("prepare current plan");

        state.finish(*generation, plan).expect("store current plan");
        state.fail(starts[0].1);
        let (stored_generation, _, stored_cancel) =
            state.get(*plan_id).expect("retrieve current plan");
        assert_eq!(stored_generation, *generation);
        assert!(Arc::ptr_eq(&stored_cancel, active_cancel));
        state.clear();
        assert!(active_cancel.load(Ordering::Acquire));
    }

    #[test]
    fn content_integrity_benchmark_completes_and_cancels_the_production_loop() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let path = temp.path().join("content.bin");
        let logical_bytes = CONTENT_INTEGRITY_CHUNK_BYTES * 3;
        fs::write(&path, vec![5_u8; logical_bytes]).expect("write content fixture");

        let complete = benchmark_content_integrity(&path, None).expect("measure complete hash");
        assert_eq!(complete.bytes_read, logical_bytes as u64);
        assert!(!complete.cancelled);

        let cancelled =
            benchmark_content_integrity(&path, Some(CONTENT_INTEGRITY_CHUNK_BYTES as u64))
                .expect("measure cancelled hash");
        assert!(cancelled.cancelled);
        assert_eq!(
            cancelled.cancel_requested_at_bytes,
            Some(CONTENT_INTEGRITY_CHUNK_BYTES as u64)
        );
        assert_eq!(cancelled.bytes_read, CONTENT_INTEGRITY_CHUNK_BYTES as u64);
        assert!(cancelled.cancellation_latency_us.is_some());
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

    #[test]
    fn measures_asynchronous_aggregation_cancellation() {
        let measurement = benchmark_aggregation_cancellation(
            AGGREGATION_CANCELLATION_CHECK_INTERVAL_NODES * 4,
            1,
        )
        .expect("measure aggregation cancellation");

        assert_eq!(
            measurement.nodes_at_request,
            AGGREGATION_CANCELLATION_CHECK_INTERVAL_NODES
        );
        assert_eq!(
            measurement.nodes_at_return,
            AGGREGATION_CANCELLATION_CHECK_INTERVAL_NODES * 2
        );
        assert!(measurement.foreground_elapsed_us >= measurement.cancellation_latency_us);
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn completed_scans_resolve_only_validated_item_paths() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let file = temp.path().join("payload.bin");
        fs::write(&file, [1_u8]).expect("write fixture file");
        let output = scanner::scan_path(temp.path(), Arc::new(AtomicBool::new(false)), |_| {})
            .expect("scan fixture");
        let state = ScanState::default();
        let (scan_id, _, _) = state.begin();
        let stale_scan_id = scan_id + 1;
        let view = state
            .complete(scan_id, output.snapshot)
            .expect("retain snapshot");
        let file_id = view
            .items
            .iter()
            .find(|item| item.name == "payload.bin")
            .expect("find fixture file")
            .id;

        assert_eq!(
            state
                .reveal_path(scan_id, file_id)
                .expect("resolve scanned item"),
            file.canonicalize().expect("canonical fixture path")
        );
        assert_eq!(
            state
                .reveal_path(stale_scan_id, file_id)
                .expect_err("reject stale scan"),
            "That scan is no longer available."
        );
        assert_eq!(
            state
                .root_path(scan_id)
                .expect("resolve completed scan root"),
            temp.path().canonicalize().expect("canonical fixture root")
        );
        assert_eq!(
            state
                .root_path(stale_scan_id)
                .expect_err("reject stale scan root"),
            "That scan is no longer available."
        );
        assert_eq!(
            state
                .compression_target(scan_id, file_id)
                .expect("resolve compression target")
                .path,
            file.canonicalize().expect("canonical fixture file")
        );
        assert_eq!(
            state
                .compression_target(stale_scan_id, file_id)
                .expect_err("reject stale compression target"),
            "That scan is no longer available."
        );
        assert_eq!(
            state
                .compression_target(scan_id, u64::MAX)
                .expect_err("reject unknown compression target"),
            "That item is not part of this scan."
        );
        assert_eq!(
            state
                .reveal_path(scan_id, u64::MAX)
                .expect_err("reject unknown item"),
            "That item is not part of this scan."
        );
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn discarding_a_completed_scan_releases_only_that_snapshot() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        fs::write(temp.path().join("payload.bin"), [1_u8]).expect("write fixture file");
        let output = scanner::scan_path(temp.path(), Arc::new(AtomicBool::new(false)), |_| {})
            .expect("scan fixture");
        let scans = ScanState::default();
        let (scan_id, _, _) = scans.begin();
        let stale_scan_id = scan_id + 1;
        scans
            .complete(scan_id, output.snapshot)
            .expect("retain snapshot");
        let retained = scans.snapshot(scan_id).expect("clone retained snapshot");
        let retained_weak = Arc::downgrade(&retained);
        drop(retained);

        let estimates = EstimateState::default();
        let (_, estimate_cancel) = estimates.begin(91);
        let searches = SearchState::default();
        let (_, search_cancel) = searches.begin(92);
        let plans = CompressionPlanState::default();
        let plan_generation = plans.generation();

        assert_eq!(
            discard_scan_state(stale_scan_id, &scans, &estimates, &searches, &plans)
                .expect_err("reject stale discard"),
            "That scan is no longer available."
        );
        assert!(retained_weak.upgrade().is_some());
        assert!(!estimate_cancel.load(Ordering::Relaxed));
        assert!(!search_cancel.load(Ordering::Relaxed));
        assert_eq!(plans.generation(), plan_generation);

        let released = discard_scan_state(scan_id, &scans, &estimates, &searches, &plans)
            .expect("discard current scan")
            .expect("detach the retained snapshot");
        assert!(retained_weak.upgrade().is_some());
        assert!(estimate_cancel.load(Ordering::Relaxed));
        assert!(search_cancel.load(Ordering::Relaxed));
        assert!(plans.generation() > plan_generation);
        assert_eq!(
            scans
                .root_path(scan_id)
                .expect_err("snapshot must be released"),
            "That scan is no longer available."
        );
        assert!(
            discard_scan_state(scan_id, &scans, &estimates, &searches, &plans)
                .expect("discarding an absent scan is idempotent")
                .is_none()
        );
        drop(released);
        assert!(retained_weak.upgrade().is_none());
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn snapshot_release_waits_for_in_flight_owners_and_drops_off_thread() {
        struct DropProbe(std::sync::mpsc::Sender<std::thread::ThreadId>);

        impl Drop for DropProbe {
            fn drop(&mut self) {
                let _ = self.0.send(std::thread::current().id());
            }
        }

        let caller = std::thread::current().id();
        let (dropped_tx, dropped_rx) = std::sync::mpsc::channel();
        let owner = Arc::new(DropProbe(dropped_tx));
        let in_flight = owner.clone();

        super::release_last_arc_on_blocking_pool(owner);
        assert!(
            dropped_rx
                .recv_timeout(std::time::Duration::from_millis(20))
                .is_err(),
            "the background owner must wait for in-flight work"
        );

        drop(in_flight);
        let drop_thread = dropped_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("background release must finish after the final in-flight owner");
        assert_ne!(drop_thread, caller);
    }

    #[cfg(feature = "desktop")]
    #[test]
    #[ignore = "opens the system file manager"]
    fn reveals_an_existing_item_in_the_system_file_manager() {
        let temp = tempfile::tempdir().expect("create fixture directory");
        let file = temp.path().join("reveal-me.txt");
        fs::write(&file, b"Cepa reveal smoke test").expect("write fixture file");

        tauri_plugin_opener::reveal_item_in_dir(&file).expect("reveal fixture file");
    }
}
