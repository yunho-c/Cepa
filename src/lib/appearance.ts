export interface AppearanceRoot {
  classList: Pick<DOMTokenList, "toggle">;
  style: Pick<CSSStyleDeclaration, "colorScheme">;
}

export interface AppearanceMediaQuery {
  matches: boolean;
  addEventListener(type: "change", listener: () => void): void;
  removeEventListener(type: "change", listener: () => void): void;
}

export interface AppearanceController {
  subscribe(listener: (dark: boolean) => void): () => void;
  toggle(): void;
  dispose(): void;
}

export function createAppearanceController(
  root: AppearanceRoot,
  mediaQuery: AppearanceMediaQuery,
  forcedDark?: boolean,
): AppearanceController {
  let override = forcedDark;
  let dark = override ?? mediaQuery.matches;
  const listeners = new Set<(dark: boolean) => void>();
  const apply = () => {
    const nextDark = override ?? mediaQuery.matches;
    root.classList.toggle("dark", nextDark);
    root.style.colorScheme = nextDark ? "dark" : "light";
    if (dark === nextDark) return;
    dark = nextDark;
    for (const listener of listeners) listener(dark);
  };

  apply();
  if (forcedDark === undefined) mediaQuery.addEventListener("change", apply);
  return {
    subscribe(listener) {
      listeners.add(listener);
      listener(dark);
      return () => { listeners.delete(listener); };
    },
    toggle() {
      override = !dark;
      apply();
    },
    dispose() {
      if (forcedDark === undefined) mediaQuery.removeEventListener("change", apply);
      listeners.clear();
    },
  };
}
