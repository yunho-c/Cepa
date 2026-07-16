use cepa_lib::{InitialResponseMeasurement, ScanBackend, ScanResult, benchmark_scan_with_backend};
use serde::Serialize;
use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ObservationReport {
    schema_version: u32,
    cepa_version: &'static str,
    environment: Environment,
    backend: &'static str,
    path: String,
    result: ScanResult,
    initial_view_ms: f64,
    progress_events: usize,
    initial_response: InitialResponseMeasurement,
    wall_ms: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Environment {
    os: &'static str,
    architecture: &'static str,
    logical_cpus: usize,
    build_profile: &'static str,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("scan observation failed: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let (path, backend) = parse_args()?;
    eprintln!(
        "observing one scan of {} using {backend}; no warmup or stability comparison",
        path.display()
    );
    let started_at = Instant::now();
    let scan = benchmark_scan_with_backend(&path, backend)?;
    let wall = started_at.elapsed();
    let initial_response = scan.measure_initial_response()?;
    let result = scan.result;

    let report = ObservationReport {
        schema_version: 2,
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
        backend: result.backend,
        path: result.root.clone(),
        result,
        initial_view_ms: scan.initial_view_ms,
        progress_events: scan.progress_events,
        initial_response,
        wall_ms: wall.as_secs_f64() * 1_000.0,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|error| format!("could not serialize observation report: {error}"))?
    );
    Ok(())
}

fn parse_args() -> Result<(PathBuf, ScanBackend), String> {
    let mut args = env::args_os().skip(1);
    let path = args.next().map(PathBuf::from).ok_or_else(usage)?;
    let backend = args
        .next()
        .map(parse_backend)
        .transpose()?
        .unwrap_or(ScanBackend::Auto);
    if args.next().is_some() {
        return Err(usage());
    }
    if !path.is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }
    Ok((path, backend))
}

fn parse_backend(value: OsString) -> Result<ScanBackend, String> {
    value
        .into_string()
        .map_err(|_| "backend must be valid UTF-8".to_string())?
        .parse()
}

fn usage() -> String {
    "usage: scan_observation <directory> [backend]".to_string()
}

#[cfg(test)]
mod tests {
    use super::parse_backend;
    use cepa_lib::ScanBackend;

    #[test]
    fn parses_supported_backends() {
        assert_eq!(
            parse_backend("mft".into()).expect("parse MFT"),
            ScanBackend::Mft
        );
        assert!(parse_backend("unknown".into()).is_err());
    }
}
