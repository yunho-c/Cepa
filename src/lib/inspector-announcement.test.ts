import { describe, expect, test } from "bun:test";
import {
  inspectorAnnouncement,
  type InspectorAnnouncementState,
} from "./inspector-announcement";

const idle: InspectorAnnouncementState = {
  itemName: "archive.zip",
  isInspectingCompression: false,
  compressionLabel: null,
  isEstimatingSavings: false,
  isCancellingEstimate: false,
  estimateLabel: null,
  hasEstimateStopError: false,
};

describe("inspector status announcements", () => {
  test("announces inspection and its result as concise atomic sentences", () => {
    expect(
      inspectorAnnouncement({ ...idle, isInspectingCompression: true }),
    ).toBe("Checking compression for archive.zip.");
    expect(
      inspectorAnnouncement({ ...idle, compressionLabel: "Not compressed" }),
    ).toBe("Compression for archive.zip: Not compressed.");
  });

  test("prioritizes live estimate work and its final bounded result", () => {
    expect(
      inspectorAnnouncement({ ...idle, isEstimatingSavings: true }),
    ).toBe("Estimating potential savings for archive.zip.");
    expect(
      inspectorAnnouncement({
        ...idle,
        isEstimatingSavings: true,
        isCancellingEstimate: true,
      }),
    ).toBe("Stopping the savings estimate for archive.zip.");
    expect(
      inspectorAnnouncement({ ...idle, estimateLabel: "2.00 GB–4.00 GB" }),
    ).toBe("Potential savings for archive.zip: 2.00 GB–4.00 GB.");
  });

  test("leaves stop failures to their focused alert", () => {
    expect(
      inspectorAnnouncement({
        ...idle,
        isEstimatingSavings: true,
        hasEstimateStopError: true,
      }),
    ).toBe("");
    expect(inspectorAnnouncement({ ...idle, itemName: null })).toBe("");
  });
});
