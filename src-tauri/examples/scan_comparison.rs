use cepa_lib::{ScanBackend, ScanResult, benchmark_scan_with_backend};
use serde::Serialize;
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const DEFAULT_PAIRS: usize = 9;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ComparisonReport {
    schema_version: u32,
    cepa_version: &'static str,
    path: String,
    environment: Environment,
    workload: Workload,
    left_backend: &'static str,
    right_backend: &'static str,
    warmup_runs_per_backend: usize,
    pairs: Vec<PairMeasurement>,
    summary: Summary,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Environment {
    os: &'static str,
    architecture: &'static str,
    logical_cpus: usize,
    build_profile: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Workload {
    files: u64,
    directories: u64,
    entries: u64,
    logical_bytes: u64,
    allocated_bytes: u64,
    skipped_entries: u64,
    skipped_filesystems: u64,
    duplicate_hard_links: u64,
    allocated_size_is_estimate: bool,
    hard_link_deduplication_supported: bool,
    same_filesystem_enforced: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunMeasurement {
    backend: &'static str,
    wall_ms: f64,
    scanner_elapsed_ms: u64,
    traversal_us: u64,
    aggregation_us: u64,
    indexing_us: u64,
    initial_view_ms: f64,
    initial_response_bytes: usize,
    initial_response_serialization_us: u64,
    snapshot_retained_bytes: usize,
    snapshot_bytes_per_entry: f64,
    snapshot_release_ms: f64,
    entries_per_second: f64,
}

struct MeasuredScan {
    result: ScanResult,
    measurement: RunMeasurement,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PairMeasurement {
    pair: usize,
    first_backend: &'static str,
    left: RunMeasurement,
    right: RunMeasurement,
    wall_change_percent: f64,
    traversal_change_percent: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BackendSummary {
    backend: &'static str,
    median_wall_ms: f64,
    min_wall_ms: f64,
    max_wall_ms: f64,
    median_entries_per_second: f64,
    median_traversal_us: f64,
    median_aggregation_us: f64,
    median_initial_view_ms: f64,
    median_initial_response_bytes: f64,
    median_snapshot_retained_bytes: f64,
    median_snapshot_release_ms: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    left: BackendSummary,
    right: BackendSummary,
    median_paired_wall_change_percent: f64,
    min_paired_wall_change_percent: f64,
    max_paired_wall_change_percent: f64,
    median_paired_traversal_change_percent: f64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("scan comparison failed: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let (path, left_backend, right_backend, pair_count) = parse_args()?;
    if left_backend == right_backend {
        return Err("left and right backends must differ".to_string());
    }

    eprintln!(
        "warming {} with {left_backend} and {right_backend}",
        path.display()
    );
    let left_warmup = benchmark_scan_with_backend(&path, left_backend)?;
    let expected = left_warmup.result.clone();
    let left_actual = left_warmup.result.backend;
    drop(left_warmup);

    let right_warmup = benchmark_scan_with_backend(&path, right_backend)?;
    validate_result(&expected, &right_warmup.result, "right warmup")?;
    let right_actual = right_warmup.result.backend;
    drop(right_warmup);
    if left_actual == right_actual {
        return Err(format!(
            "both requested backends resolved to {left_actual}; choose distinct implementations"
        ));
    }

    let mut pairs = Vec::with_capacity(pair_count);
    for pair in 1..=pair_count {
        let left_first = left_runs_first(pair);
        let (left, right) = if left_first {
            let left = measure_scan(&path, left_backend)?;
            validate_result(&expected, &left.result, &format!("pair {pair} left"))?;
            validate_backend(left_actual, &left.result, &format!("pair {pair} left"))?;
            let right = measure_scan(&path, right_backend)?;
            validate_result(&expected, &right.result, &format!("pair {pair} right"))?;
            validate_backend(right_actual, &right.result, &format!("pair {pair} right"))?;
            (left, right)
        } else {
            let right = measure_scan(&path, right_backend)?;
            validate_result(&expected, &right.result, &format!("pair {pair} right"))?;
            validate_backend(right_actual, &right.result, &format!("pair {pair} right"))?;
            let left = measure_scan(&path, left_backend)?;
            validate_result(&expected, &left.result, &format!("pair {pair} left"))?;
            validate_backend(left_actual, &left.result, &format!("pair {pair} left"))?;
            (left, right)
        };

        let wall_change_percent =
            percentage_change(left.measurement.wall_ms, right.measurement.wall_ms);
        let traversal_change_percent = percentage_change(
            left.measurement.traversal_us as f64,
            right.measurement.traversal_us as f64,
        );
        eprintln!(
            "pair {pair}/{pair_count} ({} first): {:.2} ms vs {:.2} ms ({wall_change_percent:+.1}%)",
            if left_first {
                left_actual
            } else {
                right_actual
            },
            left.measurement.wall_ms,
            right.measurement.wall_ms,
        );
        pairs.push(PairMeasurement {
            pair,
            first_backend: if left_first {
                left_actual
            } else {
                right_actual
            },
            left: left.measurement,
            right: right.measurement,
            wall_change_percent,
            traversal_change_percent,
        });
    }

    let report = ComparisonReport {
        schema_version: 1,
        cepa_version: env!("CARGO_PKG_VERSION"),
        path: expected.root.clone(),
        environment: Environment {
            os: env::consts::OS,
            architecture: env::consts::ARCH,
            logical_cpus: std::thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1),
            build_profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
        },
        workload: workload(&expected),
        left_backend: left_actual,
        right_backend: right_actual,
        warmup_runs_per_backend: 1,
        summary: summarize(&pairs, left_actual, right_actual),
        pairs,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|error| format!("could not serialize comparison report: {error}"))?
    );
    Ok(())
}

fn measure_scan(path: &Path, backend: ScanBackend) -> Result<MeasuredScan, String> {
    let started_at = Instant::now();
    let scan = benchmark_scan_with_backend(path, backend)?;
    let wall = started_at.elapsed();
    let initial_response = scan.measure_initial_response()?;
    let result = scan.result.clone();
    let entries = result.file_count.saturating_add(result.directory_count);
    let snapshot_retained_bytes = scan.snapshot_retained_bytes();
    let measurement = RunMeasurement {
        backend: result.backend,
        wall_ms: duration_ms(wall),
        scanner_elapsed_ms: result.elapsed_ms,
        traversal_us: result.traversal_us,
        aggregation_us: result.aggregation_us,
        indexing_us: result.indexing_us,
        initial_view_ms: scan.initial_view_ms,
        initial_response_bytes: initial_response.response_bytes,
        initial_response_serialization_us: initial_response.serialization_us,
        snapshot_retained_bytes,
        snapshot_bytes_per_entry: if entries == 0 {
            0.0
        } else {
            snapshot_retained_bytes as f64 / entries as f64
        },
        snapshot_release_ms: scan.measure_snapshot_release_ms(),
        entries_per_second: if wall.is_zero() {
            0.0
        } else {
            entries as f64 / wall.as_secs_f64()
        },
    };
    Ok(MeasuredScan {
        result,
        measurement,
    })
}

fn validate_result(
    expected: &ScanResult,
    observed: &ScanResult,
    label: &str,
) -> Result<(), String> {
    let mismatches = expected.accounting_mismatches(observed);
    if mismatches.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{label} no longer matches the warmed workload: {}",
            mismatches.join(", ")
        ))
    }
}

fn validate_backend(
    expected_backend: &str,
    observed: &ScanResult,
    label: &str,
) -> Result<(), String> {
    if observed.backend == expected_backend {
        Ok(())
    } else {
        Err(format!(
            "{label} resolved to {} after warming {expected_backend}",
            observed.backend
        ))
    }
}

fn workload(result: &ScanResult) -> Workload {
    Workload {
        files: result.file_count,
        directories: result.directory_count,
        entries: result.file_count.saturating_add(result.directory_count),
        logical_bytes: result.logical_bytes,
        allocated_bytes: result.allocated_bytes,
        skipped_entries: result.skipped_entries,
        skipped_filesystems: result.skipped_filesystems,
        duplicate_hard_links: result.duplicate_hard_links,
        allocated_size_is_estimate: result.allocated_size_is_estimate,
        hard_link_deduplication_supported: result.hard_link_deduplication_supported,
        same_filesystem_enforced: result.same_filesystem_enforced,
    }
}

fn summarize(
    pairs: &[PairMeasurement],
    left_backend: &'static str,
    right_backend: &'static str,
) -> Summary {
    let wall_changes: Vec<_> = pairs.iter().map(|pair| pair.wall_change_percent).collect();
    Summary {
        left: summarize_backend(left_backend, pairs.iter().map(|pair| &pair.left)),
        right: summarize_backend(right_backend, pairs.iter().map(|pair| &pair.right)),
        median_paired_wall_change_percent: median(wall_changes.iter().copied()),
        min_paired_wall_change_percent: wall_changes
            .iter()
            .copied()
            .reduce(f64::min)
            .unwrap_or_default(),
        max_paired_wall_change_percent: wall_changes
            .iter()
            .copied()
            .reduce(f64::max)
            .unwrap_or_default(),
        median_paired_traversal_change_percent: median(
            pairs.iter().map(|pair| pair.traversal_change_percent),
        ),
    }
}

fn summarize_backend<'a>(
    backend: &'static str,
    runs: impl Iterator<Item = &'a RunMeasurement>,
) -> BackendSummary {
    let runs: Vec<_> = runs.collect();
    BackendSummary {
        backend,
        median_wall_ms: median(runs.iter().map(|run| run.wall_ms)),
        min_wall_ms: runs
            .iter()
            .map(|run| run.wall_ms)
            .reduce(f64::min)
            .unwrap_or_default(),
        max_wall_ms: runs
            .iter()
            .map(|run| run.wall_ms)
            .reduce(f64::max)
            .unwrap_or_default(),
        median_entries_per_second: median(runs.iter().map(|run| run.entries_per_second)),
        median_traversal_us: median(runs.iter().map(|run| run.traversal_us as f64)),
        median_aggregation_us: median(runs.iter().map(|run| run.aggregation_us as f64)),
        median_initial_view_ms: median(runs.iter().map(|run| run.initial_view_ms)),
        median_initial_response_bytes: median(
            runs.iter().map(|run| run.initial_response_bytes as f64),
        ),
        median_snapshot_retained_bytes: median(
            runs.iter().map(|run| run.snapshot_retained_bytes as f64),
        ),
        median_snapshot_release_ms: median(runs.iter().map(|run| run.snapshot_release_ms)),
    }
}

fn parse_args() -> Result<(PathBuf, ScanBackend, ScanBackend, usize), String> {
    let mut args = env::args_os().skip(1);
    let path = args.next().map(PathBuf::from).ok_or_else(usage)?;
    let left = args
        .next()
        .map(parse_backend)
        .transpose()?
        .unwrap_or(ScanBackend::Jwalk);
    let right = args
        .next()
        .map(parse_backend)
        .transpose()?
        .unwrap_or(ScanBackend::Auto);
    let pairs = args
        .next()
        .map(|value| parse_positive_usize(value, "pairs"))
        .transpose()?
        .unwrap_or(DEFAULT_PAIRS);
    if args.next().is_some() {
        return Err(usage());
    }
    if !path.is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }
    Ok((path, left, right, pairs))
}

fn parse_backend(value: OsString) -> Result<ScanBackend, String> {
    value
        .into_string()
        .map_err(|_| "backend must be valid UTF-8".to_string())?
        .parse()
}

fn parse_positive_usize(value: OsString, name: &str) -> Result<usize, String> {
    let value = value
        .into_string()
        .map_err(|_| format!("{name} must be valid UTF-8"))?;
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("{name} must be a positive integer"))?;
    if parsed == 0 {
        return Err(format!("{name} must be greater than zero"));
    }
    Ok(parsed)
}

fn usage() -> String {
    "usage: scan_comparison <directory> [left-backend] [right-backend] [pairs]".to_string()
}

fn left_runs_first(pair: usize) -> bool {
    pair % 2 == 1
}

fn percentage_change(reference: f64, candidate: f64) -> f64 {
    if reference == 0.0 {
        0.0
    } else {
        ((candidate - reference) / reference) * 100.0
    }
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn median(values: impl Iterator<Item = f64>) -> f64 {
    let mut values: Vec<_> = values.collect();
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ScanBackend, ScanResult, left_runs_first, median, parse_backend, parse_positive_usize,
        percentage_change, validate_backend, validate_result,
    };
    use std::ffi::OsString;

    #[test]
    fn alternates_the_first_backend_by_pair() {
        assert!(left_runs_first(1));
        assert!(!left_runs_first(2));
        assert!(left_runs_first(3));
    }

    #[test]
    fn calculates_odd_and_even_medians() {
        assert_eq!(median([3.0, 1.0, 2.0].into_iter()), 2.0);
        assert_eq!(median([4.0, 1.0, 3.0, 2.0].into_iter()), 2.5);
    }

    #[test]
    fn reports_candidate_change_against_the_left_reference() {
        assert_eq!(percentage_change(100.0, 75.0), -25.0);
        assert_eq!(percentage_change(100.0, 125.0), 25.0);
        assert_eq!(percentage_change(0.0, 10.0), 0.0);
    }

    #[test]
    fn parses_backends_and_positive_pair_counts() {
        assert_eq!(
            parse_backend(OsString::from("jwalk")).expect("parse jwalk"),
            ScanBackend::Jwalk
        );
        assert_eq!(
            parse_backend(OsString::from("statx")).expect("parse statx"),
            ScanBackend::Statx
        );
        assert!(parse_backend(OsString::from("unknown")).is_err());
        assert_eq!(
            parse_positive_usize(OsString::from("9"), "pairs").expect("parse pairs"),
            9
        );
        assert!(parse_positive_usize(OsString::from("0"), "pairs").is_err());
    }

    #[test]
    fn rejects_accounting_drift_with_the_changed_field() {
        let expected = sample_result();
        let mut changed = expected.clone();
        changed.file_count += 1;
        let error = validate_result(&expected, &changed, "pair 2 right")
            .expect_err("changed workload must fail");
        assert!(error.contains("pair 2 right"));
        assert!(error.contains("file_count"));
    }

    #[test]
    fn rejects_backend_resolution_drift_after_warmup() {
        let mut observed = sample_result();
        observed.backend = "statx";
        let error = validate_backend("jwalk", &observed, "pair 3 left")
            .expect_err("changed backend must fail");
        assert!(error.contains("pair 3 left"));
        assert!(error.contains("statx"));
        assert!(error.contains("jwalk"));
    }

    fn sample_result() -> ScanResult {
        ScanResult {
            root: "/fixture".to_string(),
            display_name: "fixture".to_string(),
            backend: "jwalk",
            logical_bytes: 1,
            allocated_bytes: 1,
            file_count: 1,
            directory_count: 1,
            skipped_entries: 0,
            skipped_filesystems: 0,
            duplicate_hard_links: 0,
            traversal_us: 1,
            aggregation_us: 1,
            indexing_us: 0,
            elapsed_ms: 1,
            allocated_size_is_estimate: false,
            hard_link_deduplication_supported: true,
            same_filesystem_enforced: true,
        }
    }
}
