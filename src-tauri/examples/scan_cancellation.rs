use cepa_lib::{
    CancellationMeasurement, ScanBackend, benchmark_cancellation, benchmark_scan_with_backend,
};
use serde::Serialize;
use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

const DEFAULT_ITERATIONS: usize = 9;
const DEFAULT_CANCEL_AFTER_ENTRIES: u64 = 2_048;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CancellationReport {
    schema_version: u32,
    path: String,
    backend: &'static str,
    trigger_after_entries: u64,
    warmup_runs: usize,
    workload_entries: u64,
    runs: Vec<CancellationMeasurement>,
    summary: Summary,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    median_cancellation_latency_us: f64,
    min_cancellation_latency_us: u64,
    max_cancellation_latency_us: u64,
    median_scan_elapsed_us: f64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("cancellation benchmark failed: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let (path, backend, iterations, cancel_after_entries) = parse_args()?;
    eprintln!(
        "warming filesystem cache with {} using {backend}",
        path.display()
    );
    let warmup = benchmark_scan_with_backend(&path, backend)?;
    let workload_entries = warmup
        .result
        .file_count
        .saturating_add(warmup.result.directory_count);
    if workload_entries <= cancel_after_entries {
        return Err(format!(
            "workload has {workload_entries} entries; choose a trigger below that count"
        ));
    }
    let actual_backend = warmup.result.backend;
    let canonical_path = warmup.result.root.clone();
    drop(warmup);

    let mut runs = Vec::with_capacity(iterations);
    for iteration in 1..=iterations {
        let measurement = benchmark_cancellation(&path, backend, cancel_after_entries)?;
        eprintln!(
            "run {iteration}/{iterations}: {} us after {} entries",
            measurement.cancellation_latency_us, measurement.entries_at_request
        );
        runs.push(measurement);
    }

    let report = CancellationReport {
        schema_version: 1,
        path: canonical_path,
        backend: actual_backend,
        trigger_after_entries: cancel_after_entries,
        warmup_runs: 1,
        workload_entries,
        summary: summarize(&runs),
        runs,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|error| format!("could not serialize cancellation report: {error}"))?
    );
    Ok(())
}

fn summarize(runs: &[CancellationMeasurement]) -> Summary {
    Summary {
        median_cancellation_latency_us: median(runs.iter().map(|run| run.cancellation_latency_us)),
        min_cancellation_latency_us: runs
            .iter()
            .map(|run| run.cancellation_latency_us)
            .min()
            .unwrap_or_default(),
        max_cancellation_latency_us: runs
            .iter()
            .map(|run| run.cancellation_latency_us)
            .max()
            .unwrap_or_default(),
        median_scan_elapsed_us: median(runs.iter().map(|run| run.scan_elapsed_us)),
    }
}

fn median(values: impl Iterator<Item = u64>) -> f64 {
    let mut values: Vec<_> = values.collect();
    values.sort_unstable();
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] as f64 + values[middle] as f64) / 2.0
    } else {
        values[middle] as f64
    }
}

fn parse_args() -> Result<(PathBuf, ScanBackend, usize, u64), String> {
    let mut args = env::args_os().skip(1);
    let path = args.next().map(PathBuf::from).ok_or_else(usage)?;
    let backend = args
        .next()
        .map(parse_backend)
        .transpose()?
        .unwrap_or(ScanBackend::Jwalk);
    let iterations = args
        .next()
        .map(|value| parse_positive_usize(value, "iterations"))
        .transpose()?
        .unwrap_or(DEFAULT_ITERATIONS);
    let cancel_after_entries = args
        .next()
        .map(|value| parse_positive_u64(value, "cancel-after entries"))
        .transpose()?
        .unwrap_or(DEFAULT_CANCEL_AFTER_ENTRIES);
    if args.next().is_some() {
        return Err(usage());
    }
    if !path.is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }
    Ok((path, backend, iterations, cancel_after_entries))
}

fn parse_backend(value: OsString) -> Result<ScanBackend, String> {
    value
        .into_string()
        .map_err(|_| "backend must be valid UTF-8".to_string())?
        .parse()
}

fn parse_positive_usize(value: OsString, name: &str) -> Result<usize, String> {
    let parsed = parse_positive_u64(value, name)?;
    usize::try_from(parsed).map_err(|_| format!("{name} is too large"))
}

fn parse_positive_u64(value: OsString, name: &str) -> Result<u64, String> {
    let value = value
        .into_string()
        .map_err(|_| format!("{name} must be valid UTF-8"))?;
    let parsed = value
        .parse::<u64>()
        .map_err(|_| format!("{name} must be a positive integer"))?;
    if parsed == 0 {
        return Err(format!("{name} must be greater than zero"));
    }
    Ok(parsed)
}

fn usage() -> String {
    "usage: scan_cancellation <directory> [backend] [iterations] [cancel-after-entries]".to_string()
}

#[cfg(test)]
mod tests {
    use super::median;

    #[test]
    fn calculates_median_for_odd_and_even_samples() {
        assert_eq!(median([3, 1, 2].into_iter()), 2.0);
        assert_eq!(median([4, 1, 3, 2].into_iter()), 2.5);
    }
}
