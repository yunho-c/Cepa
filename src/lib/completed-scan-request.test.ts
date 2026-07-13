import { describe, expect, test } from "bun:test";
import { isCurrentCompletedScanRequest } from "./completed-scan-request";

describe("completed scan request ownership", () => {
  const request = { scanId: 42, sequence: 7 };

  test("accepts only the matching completed scan generation", () => {
    expect(
      isCurrentCompletedScanRequest(request, {
        scanId: 42,
        sequence: 7,
        complete: true,
      }),
    ).toBe(true);
    expect(
      isCurrentCompletedScanRequest(request, {
        scanId: 43,
        sequence: 7,
        complete: true,
      }),
    ).toBe(false);
    expect(
      isCurrentCompletedScanRequest(request, {
        scanId: 42,
        sequence: 7,
        complete: false,
      }),
    ).toBe(false);
  });

  test("rejects an older request even when a mock or reset reuses the scan ID", () => {
    expect(
      isCurrentCompletedScanRequest(request, {
        scanId: 42,
        sequence: 8,
        complete: true,
      }),
    ).toBe(false);
  });
});
