export type PrimaryModifier = "meta" | "ctrl";

export type DesktopCommand =
  | "chooseDirectory"
  | "rescan"
  | "search"
  | "navigateUp"
  | "closeSearch"
  | "closeDetails"
  | "suppress";

export type DesktopMenuCommand =
  | "chooseDirectory"
  | "rescan"
  | "search"
  | "navigateUp";

export interface DesktopMenuAvailability {
  canChooseDirectory: boolean;
  canRescan: boolean;
  canSearch: boolean;
  canNavigateUp: boolean;
}

export interface DesktopCommandContext extends DesktopMenuAvailability {
  primaryModifier: PrimaryModifier;
  searchOpen: boolean;
  detailsOpen: boolean;
}

export function desktopMenuAvailability(
  context: DesktopMenuAvailability,
): DesktopMenuAvailability {
  return {
    canChooseDirectory: context.canChooseDirectory,
    canRescan: context.canRescan,
    canSearch: context.canSearch,
    canNavigateUp: context.canNavigateUp,
  };
}

export function desktopCommandForMenu(
  command: unknown,
  context: DesktopCommandContext,
): DesktopMenuCommand | null {
  switch (command) {
    case "chooseDirectory":
      return context.canChooseDirectory ? command : null;
    case "rescan":
      return context.canRescan ? command : null;
    case "search":
      return context.canSearch ? command : null;
    case "navigateUp":
      return context.canNavigateUp ? command : null;
    default:
      return null;
  }
}

export function createDesktopMenuAvailabilitySync(
  update: (availability: DesktopMenuAvailability) => Promise<unknown>,
): (availability: DesktopMenuAvailability) => Promise<void> {
  let pending: DesktopMenuAvailability | null = null;
  let syncing: Promise<void> | null = null;

  async function drain() {
    while (pending !== null) {
      const availability = pending;
      pending = null;
      try {
        await update(availability);
      } catch {
        // Menu synchronization is secondary to the guarded command path.
      }
    }
  }

  return (availability) => {
    pending = availability;
    syncing ??= drain().finally(() => {
      syncing = null;
    });
    return syncing;
  };
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
