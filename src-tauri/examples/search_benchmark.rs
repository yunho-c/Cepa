use cepa_lib::{ScanBackend, SearchMeasurement, benchmark_scan_with_backend};
use serde::Serialize;
use std::env;
use std::path::PathBuf;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SearchBenchmarkReport {
    schema_version: u32,
    path: String,
    query: String,
    backend: &'static str,
    metric: &'static str,
    direct_children: usize,
    warmup_runs: usize,
    runs: Vec<SearchMeasurement>,
    median_elapsed_us: f64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("search benchmark failed: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let (path, query, iterations, backend, logical_size) = parse_args()?;
    let scan = benchmark_scan_with_backend(&path, backend)?;
    let direct_children = scan.root_item_count();
    let expected = scan.search_root(&query, logical_size)?;

    let mut runs = Vec::with_capacity(iterations);
    for iteration in 1..=iterations {
        let measurement = scan.search_root(&query, logical_size)?;
        if measurement.total_matches != expected.total_matches
            || measurement.returned_items != expected.returned_items
            || measurement.items_truncated != expected.items_truncated
        {
            return Err("search results changed between benchmark runs".to_string());
        }
        eprintln!(
            "run {iteration}/{iterations}: {} us, {} matches",
            measurement.elapsed_us, measurement.total_matches
        );
        runs.push(measurement);
    }

    let median_elapsed_us = median(runs.iter().map(|run| run.elapsed_us));
    let report = SearchBenchmarkReport {
        schema_version: 1,
        path: scan.result.root.clone(),
        query,
        backend: scan.result.backend,
        metric: if logical_size { "logical" } else { "allocated" },
        direct_children,
        warmup_runs: 1,
        runs,
        median_elapsed_us,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&report)
            .map_err(|error| format!("could not serialize benchmark report: {error}"))?
    );
    Ok(())
}

fn parse_args() -> Result<(PathBuf, String, usize, ScanBackend, bool), String> {
    let mut args = env::args().skip(1);
    let path = args.next().map(PathBuf::from).ok_or_else(usage)?;
    let query = args.next().ok_or_else(usage)?;
    let iterations = args
        .next()
        .ok_or_else(usage)?
        .parse::<usize>()
        .map_err(|_| "iterations must be a positive integer".to_string())?;
    if iterations == 0 {
        return Err("iterations must be greater than zero".to_string());
    }
    let backend = args.next().ok_or_else(usage)?.parse::<ScanBackend>()?;
    let logical_size = match args.next().as_deref() {
        Some("allocated") => false,
        Some("logical") => true,
        _ => return Err(usage()),
    };
    if args.next().is_some() || !path.is_dir() || query.trim().is_empty() {
        return Err(usage());
    }
    Ok((path, query, iterations, backend, logical_size))
}

fn usage() -> String {
    "usage: search_benchmark <directory> <query> <iterations> <backend> <allocated|logical>"
        .to_string()
}

fn median(values: impl Iterator<Item = u64>) -> f64 {
    let mut values = values.collect::<Vec<_>>();
    values.sort_unstable();
    let midpoint = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[midpoint - 1] as f64 + values[midpoint] as f64) / 2.0
    } else {
        values[midpoint] as f64
    }
}

#[cfg(test)]
mod tests {
    use super::median;

    #[test]
    fn calculates_odd_and_even_medians() {
        assert_eq!(median([7, 1, 3].into_iter()), 3.0);
        assert_eq!(median([8, 2, 4, 6].into_iter()), 5.0);
    }
}
