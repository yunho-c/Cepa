import { describe, expect, test } from "bun:test";
import {
  describeEntry,
  formatBackend,
  formatBytes,
  formatCompressionCapability,
  formatDuration,
  formatMetric,
  formatPercent,
  isCancellationError,
  metricBytes,
} from "./scanner";

describe("scanner presentation helpers", () => {
  test("keeps read-only compression capability distinct from a writer", () => {
    expect(
      formatCompressionCapability({
        status: "inspectOnly",
        filesystem: "apfs",
        volumeSupportsTransparentCompression: true,
        writerAvailable: false,
        algorithms: [],
        detail: "Read-only capability.",
      }),
    ).toBe("apfs compression · analysis only");
    expect(
      formatCompressionCapability({
        status: "unsupported",
        filesystem: "ext4",
        volumeSupportsTransparentCompression: false,
        writerAvailable: false,
        algorithms: [],
        detail: "Unsupported capability.",
      }),
    ).toBe("Compression unavailable on ext4");
  });

  test("formats byte and duration boundaries", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1_536)).toBe("1.50 KB");
    expect(formatDuration(999)).toBe("999 ms");
    expect(formatDuration(1_500)).toBe("1.5 s");
    expect(formatDuration(119_600)).toBe("2m 0s");
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
