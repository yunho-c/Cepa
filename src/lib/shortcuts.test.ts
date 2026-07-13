import { describe, expect, test } from "bun:test";
import {
  desktopCommandForKeydown,
  primaryModifierForPlatform,
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
});
