use cepa_lib::{ScanResult, benchmark_scan};
use serde::Serialize;
use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const DEFAULT_ITERATIONS: usize = 5;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BenchmarkReport {
    schema_version: u32,
    cepa_version: &'static str,
    backend: &'static str,
    path: String,
    environment: Environment,
    workload: Workload,
    warmup_runs: usize,
    runs: Vec<RunMeasurement>,
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
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RunMeasurement {
    iteration: usize,
    wall_ms: f64,
    scanner_elapsed_ms: u64,
    traversal_ms: u64,
    aggregation_ms: u64,
    indexing_ms: u64,
    initial_view_ms: f64,
    entries_per_second: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    median_wall_ms: f64,
    min_wall_ms: f64,
    max_wall_ms: f64,
    median_entries_per_second: f64,
    min_entries_per_second: f64,
    max_entries_per_second: f64,
    median_traversal_ms: f64,
    median_aggregation_ms: f64,
    median_indexing_ms: f64,
    median_initial_view_ms: f64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("scan benchmark failed: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let (path, iterations) = parse_args()?;

    eprintln!("warming filesystem cache with {}", path.display());
    let warmup_scan = benchmark_scan(&path)?;
    let warmup = warmup_scan.result.clone();
    let expected = ResultIdentity::from(&warmup);
    drop(warmup_scan);

    let mut runs = Vec::with_capacity(iterations);
    for iteration in 1..=iterations {
        let started_at = Instant::now();
        let scan = benchmark_scan(&path)?;
        let wall = started_at.elapsed();
        let result = &scan.result;
        expected.verify(result)?;

        let entries = result.file_count.saturating_add(result.directory_count);
        let entries_per_second = if wall.is_zero() {
            0.0
        } else {
            entries as f64 / wall.as_secs_f64()
        };
        eprintln!(
            "run {iteration}/{iterations}: {:.2} ms, {:.0} entries/s",
            duration_ms(wall),
            entries_per_second
        );

        runs.push(RunMeasurement {
            iteration,
            wall_ms: duration_ms(wall),
            scanner_elapsed_ms: result.elapsed_ms,
            traversal_ms: result.traversal_ms,
            aggregation_ms: result.aggregation_ms,
            indexing_ms: result.indexing_ms,
            initial_view_ms: scan.initial_view_ms,
            entries_per_second,
        });
    }

    let report = BenchmarkReport {
        schema_version: 2,
        cepa_version: env!("CARGO_PKG_VERSION"),
        backend: warmup.backend,
        path: warmup.root.clone(),
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
        workload: Workload {
            files: warmup.file_count,
            directories: warmup.directory_count,
            entries: warmup.file_count.saturating_add(warmup.directory_count),
            logical_bytes: warmup.logical_bytes,
            allocated_bytes: warmup.allocated_bytes,
            skipped_entries: warmup.skipped_entries,
        },
        warmup_runs: 1,
        summary: summarize(&runs),
        runs,
    };

    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|error| format!("could not serialize benchmark report: {error}"))?
    );
    Ok(())
}

fn parse_args() -> Result<(PathBuf, usize), String> {
    let mut args = env::args_os().skip(1);
    let path = args.next().map(PathBuf::from).ok_or_else(usage)?;
    let iterations = args
        .next()
        .map(|value| parse_positive_usize(value, "iterations"))
        .transpose()?
        .unwrap_or(DEFAULT_ITERATIONS);

    if args.next().is_some() {
        return Err(usage());
    }
    if !path.exists() {
        return Err(format!("{} does not exist", path.display()));
    }

    Ok((path, iterations))
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
    "usage: scan_benchmark <directory> [iterations]".to_string()
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn summarize(runs: &[RunMeasurement]) -> Summary {
    Summary {
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
        min_entries_per_second: runs
            .iter()
            .map(|run| run.entries_per_second)
            .reduce(f64::min)
            .unwrap_or_default(),
        max_entries_per_second: runs
            .iter()
            .map(|run| run.entries_per_second)
            .reduce(f64::max)
            .unwrap_or_default(),
        median_traversal_ms: median(runs.iter().map(|run| run.traversal_ms as f64)),
        median_aggregation_ms: median(runs.iter().map(|run| run.aggregation_ms as f64)),
        median_indexing_ms: median(runs.iter().map(|run| run.indexing_ms as f64)),
        median_initial_view_ms: median(runs.iter().map(|run| run.initial_view_ms)),
    }
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

struct ResultIdentity {
    root: String,
    files: u64,
    directories: u64,
    logical_bytes: u64,
    allocated_bytes: u64,
}

impl From<&ScanResult> for ResultIdentity {
    fn from(result: &ScanResult) -> Self {
        Self {
            root: result.root.clone(),
            files: result.file_count,
            directories: result.directory_count,
            logical_bytes: result.logical_bytes,
            allocated_bytes: result.allocated_bytes,
        }
    }
}

impl ResultIdentity {
    fn verify(&self, result: &ScanResult) -> Result<(), String> {
        let observed = Self::from(result);
        if self.root == observed.root
            && self.files == observed.files
            && self.directories == observed.directories
            && self.logical_bytes == observed.logical_bytes
            && self.allocated_bytes == observed.allocated_bytes
        {
            Ok(())
        } else {
            Err("the scanned workload changed between benchmark runs".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::median;

    #[test]
    fn calculates_median_for_odd_and_even_samples() {
        assert_eq!(median([3.0, 1.0, 2.0].into_iter()), 2.0);
        assert_eq!(median([4.0, 1.0, 3.0, 2.0].into_iter()), 2.5);
    }
}
