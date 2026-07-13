import { describe, expect, test } from "bun:test";
import type { ChartItem } from "./scanner";
import {
  STRESS_CHART_ITEMS,
  STRESS_LIST_ITEMS,
  countChartItems,
  createStressDirectoryView,
  createStressView,
} from "./dev-stress";

describe("development render stress fixture", () => {
  test("matches the production list and recursive chart bounds", () => {
    const view = createStressView(42, "/fixture");

    expect(view.items).toHaveLength(STRESS_LIST_ITEMS);
    expect(countChartItems(view.chartItems)).toBe(STRESS_CHART_ITEMS);
    expect(
      view.chartItems.reduce((total, item) => total + item.allocatedBytes, 0),
    ).toBe(view.allocatedBytes);
    for (const item of view.chartItems) assertChildrenCoverParent(item);
  });

  test("keeps the ranked list in descending metric order", () => {
    const items = createStressView(42, "/fixture").items;
    for (let index = 1; index < items.length; index += 1) {
      expect(items[index - 1].allocatedBytes).toBeGreaterThanOrEqual(
        items[index].allocatedBytes,
      );
    }
  });

  test("supports coherent drill-down for interactive stress checks", () => {
    const root = createStressView(42, "/fixture");
    const directory = createStressDirectoryView(root, 1);

    expect(directory.displayName).toBe("Project 01");
    expect(directory.items).toHaveLength(16);
    expect(directory.breadcrumbs.map((item) => item.name)).toEqual([
      "Stress fixture",
      "Project 01",
    ]);
  });
});

function assertChildrenCoverParent(item: ChartItem) {
  if (item.children.length === 0) return;
  expect(
    item.children.reduce((total, child) => total + child.allocatedBytes, 0),
  ).toBe(item.allocatedBytes);
  for (const child of item.children) assertChildrenCoverParent(child);
}
