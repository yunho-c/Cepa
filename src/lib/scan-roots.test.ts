import { describe, expect, test } from "bun:test";
import {
  scanRootUsedBytes,
  scanRootUsedPercent,
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
