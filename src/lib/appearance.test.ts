import { describe, expect, test } from "bun:test";
import { installSystemAppearance } from "./appearance";

function appearanceHarness(initialDark: boolean) {
  const classes = new Set<string>();
  const listeners = new Set<() => void>();
  const root = {
    classList: {
      toggle(name: string, enabled?: boolean) {
        if (enabled) classes.add(name);
        else classes.delete(name);
        return classes.has(name);
      },
    },
    style: { colorScheme: "" },
  };
  const mediaQuery = {
    matches: initialDark,
    addEventListener(_type: "change", listener: () => void) {
      listeners.add(listener);
    },
    removeEventListener(_type: "change", listener: () => void) {
      listeners.delete(listener);
    },
  };

  return {
    classes,
    listeners,
    mediaQuery,
    root,
    setDark(isDark: boolean) {
      mediaQuery.matches = isDark;
      for (const listener of listeners) listener();
    },
  };
}

describe("system appearance", () => {
  test("applies the initial system appearance and follows live changes", () => {
    const harness = appearanceHarness(false);
    const dispose = installSystemAppearance(harness.root, harness.mediaQuery);

    expect(harness.classes.has("dark")).toBe(false);
    expect(harness.root.style.colorScheme).toBe("light");
    expect(harness.listeners.size).toBe(1);

    harness.setDark(true);
    expect(harness.classes.has("dark")).toBe(true);
    expect(harness.root.style.colorScheme).toBe("dark");

    dispose();
    expect(harness.listeners.size).toBe(0);
  });

  test("supports fixed development previews without media listeners", () => {
    const darkHarness = appearanceHarness(false);
    installSystemAppearance(darkHarness.root, darkHarness.mediaQuery, true);

    expect(darkHarness.classes.has("dark")).toBe(true);
    expect(darkHarness.root.style.colorScheme).toBe("dark");
    expect(darkHarness.listeners.size).toBe(0);

    const lightHarness = appearanceHarness(true);
    installSystemAppearance(lightHarness.root, lightHarness.mediaQuery, false);

    expect(lightHarness.classes.has("dark")).toBe(false);
    expect(lightHarness.root.style.colorScheme).toBe("light");
    expect(lightHarness.listeners.size).toBe(0);
  });
});
