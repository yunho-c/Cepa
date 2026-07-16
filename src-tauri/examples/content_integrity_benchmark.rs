use cepa_lib::{
    CONTENT_INTEGRITY_CHUNK_BYTES, ContentIntegrityMeasurement, benchmark_content_integrity,
};
use serde::Serialize;
use std::env;
use std::path::PathBuf;

const DEFAULT_ITERATIONS: usize = 5;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ContentIntegrityReport {
    schema_version: u32,
    cepa_version: &'static str,
    environment: Environment,
    workload: Workload,
    warmup_runs: usize,
    complete_runs: Vec<ContentIntegrityMeasurement>,
    cancellation_runs: Vec<ContentIntegrityMeasurement>,
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
    path: String,
    logical_bytes: u64,
    chunk_bytes: usize,
    cancel_after_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    median_complete_elapsed_us: f64,
    median_complete_mib_per_second: f64,
    median_cancellation_latency_us: f64,
    max_cancellation_latency_us: u64,
    median_bytes_after_request: f64,
    max_bytes_after_request: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("content integrity benchmark failed: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let (path, iterations, cancel_after_bytes) = parse_args()?;
    let logical_bytes = path
        .metadata()
        .map_err(|error| format!("could not read benchmark file metadata: {error}"))?
        .len();
    eprintln!(
        "warming a {logical_bytes}-byte content-integrity pass before {iterations} measured pairs"
    );
    benchmark_content_integrity(&path, None)?;

    let mut complete_runs = Vec::with_capacity(iterations);
    let mut cancellation_runs = Vec::with_capacity(iterations);
    for iteration in 1..=iterations {
        let complete = benchmark_content_integrity(&path, None)?;
        let cancelled = benchmark_content_integrity(&path, Some(cancel_after_bytes))?;
        eprintln!(
            "run {iteration}/{iterations}: {} us complete, {} us cancellation",
            complete.elapsed_us,
            cancelled.cancellation_latency_us.unwrap_or_default()
        );
        complete_runs.push(complete);
        cancellation_runs.push(cancelled);
    }

    let report = ContentIntegrityReport {
        schema_version: 1,
        cepa_version: env!("CARGO_PKG_VERSION"),
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
            path: path.to_string_lossy().into_owned(),
            logical_bytes,
            chunk_bytes: CONTENT_INTEGRITY_CHUNK_BYTES,
            cancel_after_bytes,
        },
        warmup_runs: 1,
        summary: summarize(logical_bytes, &complete_runs, &cancellation_runs),
        complete_runs,
        cancellation_runs,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|error| format!("could not serialize benchmark report: {error}"))?
    );
    Ok(())
}

fn summarize(
    logical_bytes: u64,
    complete_runs: &[ContentIntegrityMeasurement],
    cancellation_runs: &[ContentIntegrityMeasurement],
) -> Summary {
    let bytes_after_request = || {
        cancellation_runs.iter().map(|run| {
            run.bytes_read
                .saturating_sub(run.cancel_requested_at_bytes.unwrap_or_default())
        })
    };
    Summary {
        median_complete_elapsed_us: median_u64(complete_runs.iter().map(|run| run.elapsed_us)),
        median_complete_mib_per_second: median_f64(complete_runs.iter().map(|run| {
            let seconds = run.elapsed_us.max(1) as f64 / 1_000_000.0;
            logical_bytes as f64 / (1024.0 * 1024.0) / seconds
        })),
        median_cancellation_latency_us: median_u64(
            cancellation_runs
                .iter()
                .filter_map(|run| run.cancellation_latency_us),
        ),
        max_cancellation_latency_us: cancellation_runs
            .iter()
            .filter_map(|run| run.cancellation_latency_us)
            .max()
            .unwrap_or_default(),
        median_bytes_after_request: median_u64(bytes_after_request()),
        max_bytes_after_request: bytes_after_request().max().unwrap_or_default(),
    }
}

fn parse_args() -> Result<(PathBuf, usize, u64), String> {
    let mut args = env::args().skip(1);
    let path = args.next().map(PathBuf::from).ok_or_else(usage)?;
    let iterations = args
        .next()
        .map(|value| parse_positive_usize(&value, "iterations"))
        .transpose()?
        .unwrap_or(DEFAULT_ITERATIONS);
    let metadata = path.metadata().map_err(|_| usage())?;
    if !metadata.is_file() {
        return Err(usage());
    }
    let logical_bytes = metadata.len();
    let cancel_after_bytes = args
        .next()
        .map(|value| parse_positive_u64(&value, "cancel-after bytes"))
        .transpose()?
        .unwrap_or(logical_bytes / 2);
    if args.next().is_some()
        || logical_bytes <= CONTENT_INTEGRITY_CHUNK_BYTES as u64
        || cancel_after_bytes == 0
        || cancel_after_bytes > logical_bytes.saturating_sub(CONTENT_INTEGRITY_CHUNK_BYTES as u64)
    {
        return Err(usage());
    }
    Ok((path, iterations, cancel_after_bytes))
}

fn parse_positive_usize(value: &str, name: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("{name} must be a positive integer"))?;
    if parsed == 0 {
        return Err(format!("{name} must be greater than zero"));
    }
    Ok(parsed)
}

fn parse_positive_u64(value: &str, name: &str) -> Result<u64, String> {
    let parsed = value
        .parse::<u64>()
        .map_err(|_| format!("{name} must be a positive integer"))?;
    if parsed == 0 {
        return Err(format!("{name} must be greater than zero"));
    }
    Ok(parsed)
}

fn usage() -> String {
    "usage: content_integrity_benchmark <file> [iterations] [cancel-after-bytes]".into()
}

fn median_u64(values: impl Iterator<Item = u64>) -> f64 {
    median_f64(values.map(|value| value as f64))
}

fn median_f64(values: impl Iterator<Item = f64>) -> f64 {
    let mut values = values.collect::<Vec<_>>();
    values.sort_unstable_by(f64::total_cmp);
    let midpoint = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[midpoint - 1] + values[midpoint]) / 2.0
    } else {
        values[midpoint]
    }
}

#[cfg(test)]
mod tests {
    use super::{median_u64, parse_positive_u64, parse_positive_usize};

    #[test]
    fn calculates_odd_and_even_medians() {
        assert_eq!(median_u64([7, 1, 3].into_iter()), 3.0);
        assert_eq!(median_u64([8, 2, 4, 6].into_iter()), 5.0);
    }

    #[test]
    fn rejects_non_positive_counts() {
        assert!(parse_positive_usize("0", "iterations").is_err());
        assert!(parse_positive_u64("many", "bytes").is_err());
    }
}
