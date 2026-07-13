import { describe, expect, test } from "bun:test";
import { listNavigationTarget } from "./list-navigation";

describe("directory list keyboard navigation", () => {
  const ids = [11, 22, 33];

  test("moves between items without wrapping past the collection bounds", () => {
    expect(listNavigationTarget(ids, 11, "ArrowDown")).toBe(22);
    expect(listNavigationTarget(ids, 22, "ArrowDown")).toBe(33);
    expect(listNavigationTarget(ids, 33, "ArrowDown")).toBe(33);
    expect(listNavigationTarget(ids, 33, "ArrowUp")).toBe(22);
    expect(listNavigationTarget(ids, 11, "ArrowUp")).toBe(11);
  });

  test("supports Home and End and recovers a missing focus target", () => {
    expect(listNavigationTarget(ids, 22, "Home")).toBe(11);
    expect(listNavigationTarget(ids, 22, "End")).toBe(33);
    expect(listNavigationTarget(ids, null, "ArrowDown")).toBe(11);
    expect(listNavigationTarget(ids, 99, "ArrowUp")).toBe(11);
  });

  test("ignores unrelated keys and empty collections", () => {
    expect(listNavigationTarget(ids, 22, "Enter")).toBeNull();
    expect(listNavigationTarget([], null, "Home")).toBeNull();
  });
});
