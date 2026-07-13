export type PrimaryModifier = "meta" | "ctrl";

export type DesktopCommand =
  | "chooseDirectory"
  | "rescan"
  | "search"
  | "navigateUp"
  | "closeSearch"
  | "closeDetails"
  | "suppress";

export interface DesktopCommandContext {
  primaryModifier: PrimaryModifier;
  canChooseDirectory: boolean;
  canRescan: boolean;
  canSearch: boolean;
  canNavigateUp: boolean;
  searchOpen: boolean;
  detailsOpen: boolean;
}

export interface ShortcutEvent {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
  shiftKey: boolean;
  defaultPrevented: boolean;
  isComposing: boolean;
}

export function primaryModifierForPlatform(platform: string): PrimaryModifier {
  return /Mac|iPhone|iPad|iPod/i.test(platform) ? "meta" : "ctrl";
}

export function desktopCommandForKeydown(
  event: ShortcutEvent,
  context: DesktopCommandContext,
): DesktopCommand | null {
  if (event.defaultPrevented || event.isComposing) return null;

  const primaryPressed =
    context.primaryModifier === "meta"
      ? event.metaKey && !event.ctrlKey
      : event.ctrlKey && !event.metaKey;
  const key = event.key.toLowerCase();

  if (primaryPressed && !event.altKey && !event.shiftKey) {
    if (key === "o") {
      return context.canChooseDirectory ? "chooseDirectory" : "suppress";
    }
    if (key === "r") return context.canRescan ? "rescan" : "suppress";
    if (key === "f") return context.canSearch ? "search" : "suppress";
  }

  if (
    event.altKey &&
    !event.metaKey &&
    !event.ctrlKey &&
    !event.shiftKey &&
    event.key === "ArrowLeft"
  ) {
    return context.canNavigateUp ? "navigateUp" : "suppress";
  }

  if (
    event.key === "Escape" &&
    !event.metaKey &&
    !event.ctrlKey &&
    !event.altKey &&
    !event.shiftKey
  ) {
    if (context.searchOpen) return "closeSearch";
    if (context.detailsOpen) return "closeDetails";
  }

  return null;
}
