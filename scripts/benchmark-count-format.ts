import { formatCount } from "../src/lib/scanner";

const DEFAULT_ITERATIONS = 9;
const DEFAULT_OPERATIONS = 10_000;

interface Measurement {
  elapsedUs: number;
  checksum: number;
}

interface Run {
  iteration: number;
  first: "perCall" | "cached";
  perCall: Measurement;
  cached: Measurement;
}

function perCallFormatCount(count: number): string {
  return new Intl.NumberFormat(undefined, { notation: "compact" }).format(count);
}

function measure(
  formatter: (count: number) => string,
  operations: number,
): Measurement {
  let checksum = 0;
  const startedAt = performance.now();
  for (let index = 0; index < operations; index += 1) {
    checksum += formatter(index * 1_001).length;
  }
  return {
    elapsedUs: Math.round((performance.now() - startedAt) * 1_000),
    checksum,
  };
}

function median(values: number[]): number {
  const sorted = [...values].sort((left, right) => left - right);
  const midpoint = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0
    ? (sorted[midpoint - 1] + sorted[midpoint]) / 2
    : sorted[midpoint];
}

function positiveInteger(value: string | undefined, fallback: number): number {
  if (value === undefined) return fallback;
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) {
    throw new Error("iterations and operations must be positive integers");
  }
  return parsed;
}

function assertEquivalent(operations: number) {
  for (let index = 0; index < operations; index += 1) {
    const value = index * 1_001;
    if (perCallFormatCount(value) !== formatCount(value)) {
      throw new Error(`formatter output changed for ${value}`);
    }
  }
}

function main() {
  const iterations = positiveInteger(Bun.argv[2], DEFAULT_ITERATIONS);
  const operations = positiveInteger(Bun.argv[3], DEFAULT_OPERATIONS);

  assertEquivalent(operations);
  measure(perCallFormatCount, operations);
  measure(formatCount, operations);

  const runs: Run[] = [];
  for (let iteration = 1; iteration <= iterations; iteration += 1) {
    const cachedFirst = iteration % 2 === 0;
    const first = cachedFirst
      ? measure(formatCount, operations)
      : measure(perCallFormatCount, operations);
    const second = cachedFirst
      ? measure(perCallFormatCount, operations)
      : measure(formatCount, operations);
    const perCall = cachedFirst ? second : first;
    const cached = cachedFirst ? first : second;
    if (perCall.checksum !== cached.checksum) {
      throw new Error("formatter output changed during the comparison");
    }
    runs.push({
      iteration,
      first: cachedFirst ? "cached" : "perCall",
      perCall,
      cached,
    });
  }

  const perCallMedianUs = median(runs.map((run) => run.perCall.elapsedUs));
  const cachedMedianUs = median(runs.map((run) => run.cached.elapsedUs));
  console.log(
    JSON.stringify(
      {
        schemaVersion: 1,
        environment: {
          os: process.platform,
          architecture: process.arch,
          bunVersion: Bun.version,
        },
        workload: { iterations, operations },
        warmupRuns: 1,
        runs,
        summary: {
          perCallMedianUs,
          cachedMedianUs,
          cachedChangePercent:
            perCallMedianUs === 0
              ? 0
              : ((cachedMedianUs - perCallMedianUs) / perCallMedianUs) * 100,
        },
      },
      null,
      2,
    ),
  );
}

try {
  main();
} catch (error) {
  console.error(`count-format benchmark failed: ${String(error)}`);
  process.exitCode = 2;
}
