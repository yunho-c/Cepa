use cepa_lib::{
    AGGREGATION_CANCELLATION_CHECK_INTERVAL_NODES, AggregationCancellationMeasurement,
    benchmark_aggregation_cancellation,
};
use serde::Serialize;
use std::env;

const DEFAULT_NODE_COUNT: usize = 1_000_000;
const DEFAULT_ITERATIONS: usize = 9;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AggregationCancellationReport {
    schema_version: u32,
    cepa_version: &'static str,
    environment: Environment,
    workload: Workload,
    warmup_runs: usize,
    runs: Vec<AggregationCancellationMeasurement>,
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
    node_count: usize,
    cancel_after_nodes: usize,
    cancellation_check_interval_nodes: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    median_cancellation_latency_us: f64,
    min_cancellation_latency_us: u64,
    max_cancellation_latency_us: u64,
    median_foreground_elapsed_us: f64,
    median_nodes_after_request: f64,
    max_nodes_after_request: usize,
    median_background_release_us: f64,
    max_background_release_us: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("aggregation cancellation benchmark failed: {error}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let (node_count, iterations, cancel_after_nodes) = parse_args()?;
    eprintln!(
        "warming a {node_count}-node synthetic aggregation arena before {iterations} measured runs"
    );
    benchmark_aggregation_cancellation(node_count, cancel_after_nodes)?;

    let mut runs = Vec::with_capacity(iterations);
    for iteration in 1..=iterations {
        let measurement = benchmark_aggregation_cancellation(node_count, cancel_after_nodes)?;
        eprintln!(
            "run {iteration}/{iterations}: {} us cancellation, {} nodes after request",
            measurement.cancellation_latency_us,
            measurement
                .nodes_at_return
                .saturating_sub(measurement.nodes_at_request)
        );
        runs.push(measurement);
    }

    let report = AggregationCancellationReport {
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
            node_count,
            cancel_after_nodes,
            cancellation_check_interval_nodes: AGGREGATION_CANCELLATION_CHECK_INTERVAL_NODES,
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

fn summarize(runs: &[AggregationCancellationMeasurement]) -> Summary {
    let nodes_after_request = || {
        runs.iter()
            .map(|run| run.nodes_at_return.saturating_sub(run.nodes_at_request))
    };
    Summary {
        median_cancellation_latency_us: median_u64(
            runs.iter().map(|run| run.cancellation_latency_us),
        ),
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
        median_foreground_elapsed_us: median_u64(runs.iter().map(|run| run.foreground_elapsed_us)),
        median_nodes_after_request: median_usize(nodes_after_request()),
        max_nodes_after_request: nodes_after_request().max().unwrap_or_default(),
        median_background_release_us: median_u64(runs.iter().map(|run| run.background_release_us)),
        max_background_release_us: runs
            .iter()
            .map(|run| run.background_release_us)
            .max()
            .unwrap_or_default(),
    }
}

fn parse_args() -> Result<(usize, usize, usize), String> {
    let mut args = env::args().skip(1);
    let node_count = args
        .next()
        .map(|value| parse_positive_usize(&value, "node count"))
        .transpose()?
        .unwrap_or(DEFAULT_NODE_COUNT);
    let iterations = args
        .next()
        .map(|value| parse_positive_usize(&value, "iterations"))
        .transpose()?
        .unwrap_or(DEFAULT_ITERATIONS);
    let cancel_after_nodes = args
        .next()
        .map(|value| parse_positive_usize(&value, "cancel-after nodes"))
        .transpose()?
        .unwrap_or_else(|| node_count / 2 + 1);
    if args.next().is_some() {
        return Err(usage());
    }
    if node_count <= AGGREGATION_CANCELLATION_CHECK_INTERVAL_NODES {
        return Err(format!(
            "node count must exceed {}",
            AGGREGATION_CANCELLATION_CHECK_INTERVAL_NODES
        ));
    }
    if cancel_after_nodes >= node_count {
        return Err("cancel-after nodes must be lower than node count".to_string());
    }
    Ok((node_count, iterations, cancel_after_nodes))
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

fn usage() -> String {
    "usage: aggregation_cancellation [node-count] [iterations] [cancel-after-nodes]".to_string()
}

fn median_u64(values: impl Iterator<Item = u64>) -> f64 {
    median(values.map(|value| value as f64))
}

fn median_usize(values: impl Iterator<Item = usize>) -> f64 {
    median(values.map(|value| value as f64))
}

fn median(values: impl Iterator<Item = f64>) -> f64 {
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
    use super::{median_u64, parse_positive_usize};

    #[test]
    fn calculates_odd_and_even_medians() {
        assert_eq!(median_u64([7, 1, 3].into_iter()), 3.0);
        assert_eq!(median_u64([8, 2, 4, 6].into_iter()), 5.0);
    }

    #[test]
    fn rejects_non_positive_counts() {
        assert!(parse_positive_usize("0", "nodes").is_err());
        assert!(parse_positive_usize("many", "nodes").is_err());
    }
}
