import { describe, expect, test } from "bun:test";
import {
  scanRootActionState,
  scanRootUsedBytes,
  scanRootUsedPercent,
  scanRootsPreviewStatus,
  scanRootsRetryFocusTarget,
  shouldShowScanRoots,
  type ScanRoot,
} from "./scan-roots";

const root: ScanRoot = {
  name: "System",
  path: "/",
  displayPath: "/",
  totalBytes: 1_000,
  availableBytes: 250,
  isRemovable: false,
  isReadOnly: false,
};

describe("scan root presentation", () => {
  test("recognizes only bounded development preview states", () => {
    expect(scanRootsPreviewStatus("preview")).toBe("ready");
    expect(scanRootsPreviewStatus("ready")).toBe("ready");
    expect(scanRootsPreviewStatus("loading")).toBe("loading");
    expect(scanRootsPreviewStatus("error")).toBe("error");
    expect(scanRootsPreviewStatus("unknown")).toBeNull();
    expect(scanRootsPreviewStatus(null)).toBeNull();
  });

  test("shows the picker only when it has a visible state", () => {
    expect(shouldShowScanRoots("idle", 0)).toBe(false);
    expect(shouldShowScanRoots("ready", 0)).toBe(false);
    expect(shouldShowScanRoots("loading", 0)).toBe(true);
    expect(shouldShowScanRoots("error", 0)).toBe(true);
    expect(shouldShowScanRoots("ready", 1)).toBe(true);
  });

  test("restores retry focus through loading and every terminal state", () => {
    expect(scanRootsRetryFocusTarget("loading", 0)).toBe("loading");
    expect(scanRootsRetryFocusTarget("ready", 2)).toBe("root");
    expect(scanRootsRetryFocusTarget("error", 0)).toBe("retry");
    expect(scanRootsRetryFocusTarget("ready", 0)).toBe("fallback");
  });

  test("keeps only the selected volume focusable while its root is prepared", () => {
    expect(scanRootActionState("/", null, false)).toBe("available");
    expect(scanRootActionState("/", "/", true)).toBe("preparing");
    expect(scanRootActionState("/Volumes/Archive", "/", true)).toBe("disabled");
  });

  test("derives used capacity from available bytes", () => {
    expect(scanRootUsedBytes(root)).toBe(750);
    expect(scanRootUsedPercent(root)).toBe(75);
  });

  test("bounds inconsistent and unavailable capacity values", () => {
    expect(scanRootUsedBytes({ ...root, availableBytes: 1_100 })).toBe(0);
    expect(scanRootUsedPercent({ ...root, availableBytes: -100 })).toBe(100);
    expect(scanRootUsedPercent({ ...root, totalBytes: 0 })).toBe(0);
    expect(scanRootUsedPercent({ ...root, totalBytes: Number.NaN })).toBe(0);
  });
});
