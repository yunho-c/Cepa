import { describe, expect, test } from "bun:test";
import {
  describeEntry,
  formatBackend,
  formatBytes,
  formatCompressionState,
  formatDuration,
  formatMetric,
  formatPercent,
  formatSavingsEstimate,
  formatUnavailableItems,
  isCancellationError,
  metricBytes,
  scanProgressPresentation,
} from "./scanner";

describe("scanner presentation helpers", () => {
  test("keeps placeholder scan progress out of live announcements", () => {
    expect(
      scanProgressPresentation(
        { phase: "scanning", entriesScanned: 0, allocatedBytes: 0 },
        false,
      ),
    ).toEqual({
      statusLabel: "Scanning",
      totalLabel: "Found so far",
      currentLabel: null,
      announcement: "",
    });
    expect(
      scanProgressPresentation(
        { phase: "scanning", entriesScanned: 1, allocatedBytes: 0 },
        false,
      ).announcement,
    ).toBe("Scanned 1 entry and 0 B.");
  });

  test("preserves finishing context while a stop request is pending", () => {
    const progress = {
      phase: "finishing" as const,
      entriesScanned: 18_432,
      allocatedBytes: 302_795_292_672,
    };
    expect(scanProgressPresentation(progress, false)).toEqual({
      statusLabel: "Finishing",
      totalLabel: "Space found",
      currentLabel: "Preparing results…",
      announcement: "Finishing the scan after 18K entries.",
    });
    expect(scanProgressPresentation(progress, true)).toEqual({
      statusLabel: "Stopping",
      totalLabel: "Space found",
      currentLabel: "Preparing results…",
      announcement: "Stopping while preparing results for 18K entries.",
    });
    expect(
      scanProgressPresentation({ ...progress, phase: "scanning" }, true),
    ).toEqual({
      statusLabel: "Stopping",
      totalLabel: "Found so far",
      currentLabel: null,
      announcement: "Stopping after 18K entries.",
    });
  });

  test("labels existing-data state separately from future-write policy", () => {
    expect(
      formatCompressionState({
        state: "compressed",
        scope: "existingData",
        format: "decmpfs",
        detail: "Current data state.",
      }),
    ).toBe("Compressed · decmpfs");
    expect(
      formatCompressionState({
        state: "enabled",
        scope: "futureWrites",
        format: null,
        detail: "Future writes only.",
      }),
    ).toBe("Enabled for future writes");
    expect(
      formatCompressionState({
        state: "inherited",
        scope: "futureWrites",
        format: null,
        detail: "Inherited policy.",
      }),
    ).toBe("Following filesystem policy");
    expect(
      formatCompressionState({
        state: "unavailable",
        scope: "none",
        format: null,
        detail: "Inspection failed.",
      }),
    ).toBe("Couldn’t be checked");
  });

  test("formats savings as a bounded range rather than a guarantee", () => {
    const estimate = {
      status: "estimated" as const,
      algorithm: "zlib-proxy",
      fidelity: "proxy" as const,
      confidence: "low" as const,
      sampledBytes: 786_432,
      logicalBytes: 10_737_418_240,
      allocatedBytes: 10_737_418_240,
      estimatedSavingsLower: 2_147_483_648,
      estimatedSavingsUpper: 4_294_967_296,
      estimatorVersion: 1,
      detail: "Bounded proxy estimate.",
    };
    expect(formatSavingsEstimate(estimate)).toBe("2.00 GB–4.00 GB");
    expect(
      formatSavingsEstimate({
        ...estimate,
        estimatedSavingsUpper: estimate.estimatedSavingsLower,
      }),
    ).toBe("2.00 GB");
    expect(
      formatSavingsEstimate({
        ...estimate,
        estimatedSavingsLower: 0,
        estimatedSavingsUpper: 0,
      }),
    ).toBe("No likely savings");
    expect(
      formatSavingsEstimate({
        ...estimate,
        status: "unavailable",
        estimatedSavingsLower: null,
        estimatedSavingsUpper: null,
      }),
    ).toBe("Couldn’t be estimated");
    expect(
      formatSavingsEstimate({
        ...estimate,
        status: "unsupported",
        estimatedSavingsLower: null,
        estimatedSavingsUpper: null,
      }),
    ).toBe("Not supported");
  });

  test("formats byte and duration boundaries", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1_536)).toBe("1.50 KB");
    expect(formatDuration(999)).toBe("999 ms");
    expect(formatDuration(1_500)).toBe("1.5 s");
    expect(formatDuration(119_600)).toBe("2m 0s");
  });

  test("describes unavailable items without implying a single failure cause", () => {
    expect(formatUnavailableItems(1)).toBe(
      "1 item was unavailable during this scan",
    );
    expect(formatUnavailableItems(2)).toBe(
      "2 items were unavailable during this scan",
    );
    expect(formatUnavailableItems(Number.NaN)).toBe(
      "0 items were unavailable during this scan",
    );
  });

  test("bounds percentages and handles empty totals", () => {
    expect(formatPercent(1, 4)).toBe("25.0%");
    expect(formatPercent(8, 4)).toBe("100.0%");
    expect(formatPercent(-1, 4)).toBe("0.0%");
    expect(formatPercent(1, 0)).toBe("0.0%");
  });

  test("labels native and portable backends accurately", () => {
    expect(formatBackend("jwalk")).toBe("Portable");
    expect(formatBackend("getattrlistbulk")).toBe("macOS native");
    expect(formatBackend("mft")).toBe("Windows native");
    expect(formatBackend("statx")).toBe("Linux native");
  });

  test("selects and labels the requested size metric", () => {
    const entry = { allocatedBytes: 12, logicalBytes: 48 };
    expect(metricBytes(entry, "allocated")).toBe(12);
    expect(metricBytes(entry, "logical")).toBe(48);
    expect(formatMetric("allocated")).toBe("Space on disk");
    expect(formatMetric("logical")).toBe("Logical size");
  });

  test("recognizes cancellation errors without matching case", () => {
    expect(isCancellationError("Scan cancelled.")).toBe(true);
    expect(isCancellationError(new Error("CANCELLED by user"))).toBe(true);
    expect(isCancellationError("Permission denied")).toBe(false);
  });

  test("describes entries according to filesystem semantics", () => {
    expect(
      describeEntry({ kind: "directory", fileCount: 12, directoryCount: 3 }),
    ).toContain("files");
    expect(describeEntry({ kind: "file", fileCount: 1, directoryCount: 0 })).toBe(
      "File",
    );
    expect(
      describeEntry({ kind: "symlink", fileCount: 0, directoryCount: 0 }),
    ).toBe("Symbolic link · not followed");
  });
});
