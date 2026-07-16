import { describe, expect, test } from "bun:test";
import {
  scanEntryRecoveryMessage,
  scanFailurePresentation,
} from "./recovery-copy";

describe("recovery copy", () => {
  test("keeps folder entry guidance actionable without exposing implementation details", () => {
    expect(scanEntryRecoveryMessage("folder")).toBe(
      "Choose another folder or check that this one is still available.",
    );
    expect(scanEntryRecoveryMessage("picker")).toBe(
      "Try opening the folder picker again, or enter a path instead.",
    );
    expect(scanEntryRecoveryMessage("drop")).toBe(
      "Drop a single folder and try again.",
    );
  });

  test("distinguishes a rejected start from a failed in-progress scan", () => {
    expect(scanFailurePresentation(false)).toEqual({
      heading: "That scan didn’t start.",
      guidance: "Check the folder and try again.",
    });
    expect(scanFailurePresentation(true)).toEqual({
      heading: "The scan couldn’t finish.",
      guidance: "No results were saved. Choose another folder or try again.",
    });
  });
});
