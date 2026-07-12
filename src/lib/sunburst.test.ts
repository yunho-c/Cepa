import { describe, expect, test } from "bun:test";
import type { ChartItem } from "./scanner";
import { createSunburst } from "./sunburst";

function item(
  id: number,
  allocatedBytes: number,
  children: ChartItem[] = [],
): ChartItem {
  return {
    id,
    name: `item-${id}`,
    kind: "directory",
    logicalBytes: allocatedBytes,
    allocatedBytes,
    children,
  };
}

describe("sunburst geometry", () => {
  test("creates finite paths for nested weighted items", () => {
    const segments = createSunburst([
      item(1, 75, [item(3, 25)]),
      item(2, 25),
    ]);

    expect(segments).toHaveLength(3);
    expect(segments.map((segment) => segment.depth)).toEqual([0, 1, 0]);
    for (const segment of segments) {
      expect(segment.pathData).not.toContain("NaN");
      expect(segment.pathData).not.toContain("Infinity");
    }
  });

  test("uses a minimum weight for zero-byte entries", () => {
    expect(createSunburst([item(1, 0), item(2, 0)])).toHaveLength(2);
  });

  test("returns no segments for an empty view", () => {
    expect(createSunburst([])).toEqual([]);
  });
});
