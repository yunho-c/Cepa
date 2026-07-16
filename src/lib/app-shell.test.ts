import { describe, expect, test } from "bun:test";
import { brandActsAsHome, type AppStatus } from "./app-shell";

describe("desktop shell presentation", () => {
  test("offers the brand as Home only while a completed snapshot exists", () => {
    const nonResultStates: AppStatus[] = [
      "idle",
      "scanning",
      "cancelling",
      "cancelled",
      "error",
    ];

    for (const status of nonResultStates) {
      expect(brandActsAsHome(status)).toBe(false);
    }
    expect(brandActsAsHome("complete")).toBe(true);
  });
});
