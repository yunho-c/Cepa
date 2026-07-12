use cepa_lib::{ScanBackend, ScanResult, benchmark_scan_with_backend};
use serde::Serialize;
use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ParityReport {
    schema_version: u32,
    path: String,
    matched: bool,
    mismatches: Vec<String>,
    left: ScanResult,
    right: ScanResult,
}

fn main() {
    match run() {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(error) => {
            eprintln!("scan parity validation failed: {error}");
            std::process::exit(2);
        }
    }
}

fn run() -> Result<bool, String> {
    let (path, left_backend, right_backend) = parse_args()?;
    eprintln!("scanning {} with {left_backend}", path.display());
    let left = benchmark_scan_with_backend(&path, left_backend)?.result;
    eprintln!("scanning {} with {right_backend}", path.display());
    let right = benchmark_scan_with_backend(&path, right_backend)?.result;
    let mismatches = compare_results(&left, &right);
    let matched = mismatches.is_empty();

    let report = ParityReport {
        schema_version: 1,
        path: left.root.clone(),
        matched,
        mismatches,
        left,
        right,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|error| format!("could not serialize parity report: {error}"))?
    );
    Ok(matched)
}

fn compare_results(left: &ScanResult, right: &ScanResult) -> Vec<String> {
    left.accounting_mismatches(right)
        .into_iter()
        .map(str::to_string)
        .collect()
}

fn parse_args() -> Result<(PathBuf, ScanBackend, ScanBackend), String> {
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
    if args.next().is_some() {
        return Err(usage());
    }
    if !path.is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }
    Ok((path, left, right))
}

fn parse_backend(value: OsString) -> Result<ScanBackend, String> {
    value
        .into_string()
        .map_err(|_| "backend must be valid UTF-8".to_string())?
        .parse()
}

fn usage() -> String {
    "usage: scan_parity <directory> [left-backend] [right-backend]".to_string()
}
