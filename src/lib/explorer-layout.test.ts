import { describe, expect, test } from "bun:test";
import { explorerSplit, explorerSplitKey } from "./explorer-layout";

describe("explorer divider bounds", () => {
  test("preserves the default desktop split and room for the list", () => {
    const split = explorerSplit(832, 880);
    expect(split.width).toBe(352);
    expect(split.min).toBe(320);
    expect(split.max).toBe(511);
    expect(explorerSplit(832, 880, 800).width).toBe(511);
    expect(explorerSplit(832, 880, 100).width).toBe(320);
    expect(explorerSplit(1536, 1600, 900).width).toBe(900);
    expect(explorerSplit(1536, 1600, 1200).width).toBe(1040);
    expect(explorerSplit(2512, 2560).width).toBe(1024);
    expect(explorerSplit(3096, 3200).width).toBe(1040);
  });

  test("keeps compact panes usable at the supported minimum", () => {
    const split = explorerSplit(588, 620);
    expect(split.width).toBeCloseTo(211.32);
    expect(split.min).toBe(196);
    expect(split.max).toBe(287);
    expect(split.available - split.max).toBe(300);
    expect(explorerSplit(588, 620, 500).width).toBe(287);
  });

  test("clamps a preferred width without destroying the user's wider-window choice", () => {
    const preferred = 480;
    expect(explorerSplit(832, 880, preferred).width).toBe(preferred);
    expect(explorerSplit(588, 620, preferred).width).toBe(287);
    expect(explorerSplit(832, 880, preferred).width).toBe(preferred);
    expect(explorerSplit(1200, 1280).width).toBe(512);
  });

  test("handles the stacked breakpoint and pre-measurement width", () => {
    expect(explorerSplit(500, 559).stacked).toBe(true);
    expect(explorerSplit(528, 560).stacked).toBe(false);
    expect(explorerSplit(0, 880)).toMatchObject({ available: 0, min: 0, max: 0, width: 0 });
    for (const viewport of [560, 620, 720, 721, 880, 1280, 2560]) {
      const split = explorerSplit(viewport - 48, viewport, 900);
      expect(split.min).toBeLessThanOrEqual(split.width);
      expect(split.width).toBeLessThanOrEqual(split.max);
      expect(split.max).toBeLessThanOrEqual(1040);
      expect(split.available - split.width).toBeGreaterThanOrEqual(viewport <= 720 ? 300 : 320);
    }
  });

  test("supports bounded keyboard steps, limits, and reset", () => {
    const split = explorerSplit(832, 880, 400);
    expect(explorerSplitKey("ArrowLeft", split, false)).toBe(390);
    expect(explorerSplitKey("ArrowRight", split, true)).toBe(440);
    expect(explorerSplitKey("Home", split, false)).toBe(320);
    expect(explorerSplitKey("End", split, false)).toBe(511);
    expect(explorerSplitKey("ArrowRight", { ...split, width: 510 }, true)).toBe(511);
    expect(explorerSplitKey("ArrowLeft", { ...split, width: 321 }, true)).toBe(320);
    expect(explorerSplitKey("Enter", split, false)).toBeNull();
    expect(explorerSplitKey("Tab", split, false)).toBeUndefined();
  });
});
