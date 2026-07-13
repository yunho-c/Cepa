import { describe, expect, test } from "bun:test";
import {
  createDesktopMenuAvailabilitySync,
  desktopCommandForMenu,
  desktopCommandForKeydown,
  desktopMenuAvailability,
  primaryModifierForPlatform,
  scanPathPlaceholderForPlatform,
  type DesktopCommandContext,
  type ShortcutEvent,
} from "./shortcuts";

const baseContext: DesktopCommandContext = {
  primaryModifier: "meta",
  canChooseDirectory: true,
  canRescan: true,
  canSearch: true,
  canNavigateUp: true,
  searchOpen: false,
  detailsOpen: false,
};

function keyEvent(key: string, overrides: Partial<ShortcutEvent> = {}): ShortcutEvent {
  return {
    key,
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    defaultPrevented: false,
    isComposing: false,
    ...overrides,
  };
}

describe("desktop shortcuts", () => {
  test("uses the platform-native primary modifier", () => {
    expect(primaryModifierForPlatform("MacIntel")).toBe("meta");
    expect(primaryModifierForPlatform("Win32")).toBe("ctrl");
    expect(primaryModifierForPlatform("Linux x86_64")).toBe("ctrl");
  });

  test("shows a native path example for manual folder entry", () => {
    expect(scanPathPlaceholderForPlatform("MacIntel")).toBe("/Users/you/Documents");
    expect(scanPathPlaceholderForPlatform("Win32")).toBe(
      "C:\\Users\\you\\Documents",
    );
    expect(scanPathPlaceholderForPlatform("Linux x86_64")).toBe(
      "/home/you/Documents",
    );
  });

  test("maps open, rescan, and search without hijacking modified variants", () => {
    expect(desktopCommandForKeydown(keyEvent("o", { metaKey: true }), baseContext)).toBe(
      "chooseDirectory",
    );
    expect(desktopCommandForKeydown(keyEvent("r", { metaKey: true }), baseContext)).toBe(
      "rescan",
    );
    expect(desktopCommandForKeydown(keyEvent("f", { metaKey: true }), baseContext)).toBe(
      "search",
    );
    expect(
      desktopCommandForKeydown(
        keyEvent("r", { metaKey: true, shiftKey: true }),
        baseContext,
      ),
    ).toBeNull();
    expect(desktopCommandForKeydown(keyEvent("f", { ctrlKey: true }), baseContext)).toBeNull();
  });

  test("suppresses reserved webview commands when the app action is unavailable", () => {
    expect(
      desktopCommandForKeydown(keyEvent("r", { metaKey: true }), {
        ...baseContext,
        canRescan: false,
      }),
    ).toBe("suppress");
    expect(
      desktopCommandForKeydown(keyEvent("f", { metaKey: true }), {
        ...baseContext,
        canSearch: false,
      }),
    ).toBe("suppress");
  });

  test("preserves composition and already handled events", () => {
    expect(
      desktopCommandForKeydown(
        keyEvent("o", { metaKey: true, isComposing: true }),
        baseContext,
      ),
    ).toBeNull();
    expect(
      desktopCommandForKeydown(
        keyEvent("o", { metaKey: true, defaultPrevented: true }),
        baseContext,
      ),
    ).toBeNull();
  });

  test("navigates up only for an unmodified platform back shortcut", () => {
    expect(
      desktopCommandForKeydown(keyEvent("ArrowLeft", { altKey: true }), baseContext),
    ).toBe("navigateUp");
    expect(
      desktopCommandForKeydown(
        keyEvent("ArrowLeft", { altKey: true, metaKey: true }),
        baseContext,
      ),
    ).toBeNull();
    expect(
      desktopCommandForKeydown(keyEvent("ArrowLeft", { altKey: true }), {
        ...baseContext,
        canNavigateUp: false,
      }),
    ).toBe("suppress");
  });

  test("dismisses search before item details", () => {
    expect(
      desktopCommandForKeydown(keyEvent("Escape"), {
        ...baseContext,
        searchOpen: true,
        detailsOpen: true,
      }),
    ).toBe("closeSearch");
    expect(
      desktopCommandForKeydown(keyEvent("Escape"), {
        ...baseContext,
        detailsOpen: true,
      }),
    ).toBe("closeDetails");
  });

  test("projects only native menu availability fields", () => {
    expect(desktopMenuAvailability(baseContext)).toEqual({
      canChooseDirectory: true,
      canRescan: true,
      canSearch: true,
      canNavigateUp: true,
    });
  });

  test("accepts supported native menu commands only while available", () => {
    expect(desktopCommandForMenu("chooseDirectory", baseContext)).toBe(
      "chooseDirectory",
    );
    expect(desktopCommandForMenu("search", baseContext)).toBe("search");
    expect(
      desktopCommandForMenu("search", { ...baseContext, canSearch: false }),
    ).toBeNull();
    expect(desktopCommandForMenu("closeDetails", baseContext)).toBeNull();
    expect(desktopCommandForMenu({ command: "rescan" }, baseContext)).toBeNull();
  });

  test("serializes native menu updates and coalesces pending state", async () => {
    let releaseFirstUpdate: (() => void) | undefined;
    const firstUpdate = new Promise<void>((resolve) => {
      releaseFirstUpdate = resolve;
    });
    const updates: boolean[] = [];
    const sync = createDesktopMenuAvailabilitySync(async (availability) => {
      updates.push(availability.canRescan);
      if (updates.length === 1) await firstUpdate;
    });

    const settled = sync({ ...baseContext, canRescan: false });
    void sync({ ...baseContext, canRescan: false });
    void sync({ ...baseContext, canRescan: true });
    expect(updates).toEqual([false]);

    releaseFirstUpdate?.();
    await settled;
    expect(updates).toEqual([false, true]);
  });
});
