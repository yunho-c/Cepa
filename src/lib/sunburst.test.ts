import { describe, expect, test } from "bun:test";
import type { ChartItem } from "./scanner";
import { createSunburst, sunburstBranches, sunburstNavigationTarget } from "./sunburst";

function item(
  id: number,
  allocatedBytes: number,
  logicalBytes = allocatedBytes,
  children: ChartItem[] = [],
): ChartItem {
  return {
    id,
    name: `item-${id}`,
    kind: "directory",
    logicalBytes,
    allocatedBytes,
    children,
  };
}

describe("sunburst geometry", () => {
  test("creates finite paths for nested weighted items", () => {
    const segments = createSunburst([
      item(1, 75, 75, [item(3, 25)]),
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

  test("keeps all descendants in their root branch color", () => {
    const items = [item(1, 80, 80, [item(3, 50, 50, [item(5, 50)]), item(4, 30)]), item(2, 20)];
    const branches = sunburstBranches(items);
    const segments = createSunburst(items);
    for (const id of [1, 3, 4, 5]) {
      expect(branches.get(id)).toEqual({ id: 1, colorIndex: 0 });
      expect(segments.find(segment => segment.item.id === id)).toMatchObject({ branchId: 1, colorIndex: 0 });
    }
    expect(branches.get(2)).toEqual({ id: 2, colorIndex: 1 });
    expect(segments.find(segment => segment.item.id === 2)).toMatchObject({ branchId: 2, colorIndex: 1 });
  });

  test("retains branch identity for tiny entries and filtered row subsets", () => {
    const items = [item(1, 1_000_000), item(2, 1)];
    const branches = sunburstBranches(items);
    expect(createSunburst(items).some(segment => segment.item.id === 2)).toBe(false);
    // A search result uses IDs against the original chart tree, never its new rank.
    expect([2].map(id => branches.get(id))).toEqual([{ id: 2, colorIndex: 1 }]);
    expect(branches.get(999)).toBeUndefined();
  });

  test("keeps root aggregates neutral and nested aggregates in their containing branch", () => {
    const aggregate: ChartItem = { ...item(9, 25), id: null, kind: "other" };
    const items = [item(1, 75, 75, [item(2, 50), aggregate]), aggregate];
    const branches = sunburstBranches(items);
    const segments = createSunburst(items);
    expect(branches.size).toBe(2);
    expect(segments.find(segment => segment.item.id === null && segment.depth === 0))
      .toMatchObject({ branchId: null, colorIndex: null });
    expect(segments.find(segment => segment.item.id === null && segment.depth === 1))
      .toMatchObject({ branchId: 1, colorIndex: 0 });
  });

  test("changes geometry with the selected metric", () => {
    const items = [item(1, 90, 10), item(2, 10, 90)];
    const allocated = createSunburst(items, "allocated");
    const logical = createSunburst(items, "logical");

    expect(allocated[0].pathData).not.toBe(logical[0].pathData);
    expect(allocated[1].pathData).not.toBe(logical[1].pathData);
  });

  test("gives repeated aggregate labels unique stable keys", () => {
    const aggregate = (): ChartItem => ({
      id: null,
      name: "16 more items",
      kind: "other",
      logicalBytes: 25,
      allocatedBytes: 25,
      children: [],
    });
    const items = [
      item(1, 50, 50, [aggregate()]),
      item(2, 50, 50, [aggregate()]),
    ];

    const first = createSunburst(items);
    const second = createSunburst(items);
    const keys = first.map((segment) => segment.key);

    expect(new Set(keys).size).toBe(keys.length);
    expect(second.map((segment) => segment.key)).toEqual(keys);
  });

  test("moves through interactive segments with one wrapping focus target", () => {
    const segments = createSunburst([
      item(1, 75, 75, [item(2, 25)]),
      item(3, 25),
    ]);

    expect(sunburstNavigationTarget(segments, 1, "ArrowRight")).toBe(2);
    expect(sunburstNavigationTarget(segments, 2, "ArrowDown")).toBe(3);
    expect(sunburstNavigationTarget(segments, 3, "ArrowRight")).toBe(1);
    expect(sunburstNavigationTarget(segments, 1, "ArrowLeft")).toBe(3);
    expect(sunburstNavigationTarget(segments, 2, "ArrowUp")).toBe(1);
  });

  test("supports Home and End while ignoring aggregate and unrelated keys", () => {
    const segments = createSunburst([
      item(1, 75),
      {
        id: null,
        name: "More items",
        kind: "other",
        logicalBytes: 25,
        allocatedBytes: 25,
        children: [],
      },
    ]);

    expect(sunburstNavigationTarget(segments, null, "Home")).toBe(1);
    expect(sunburstNavigationTarget(segments, null, "End")).toBe(1);
    expect(sunburstNavigationTarget(segments, null, "ArrowRight")).toBe(1);
    expect(sunburstNavigationTarget(segments, 1, "Enter")).toBeNull();
    expect(sunburstNavigationTarget([], null, "Home")).toBeNull();
  });
});
