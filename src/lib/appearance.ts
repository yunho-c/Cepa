export interface AppearanceRoot {
  classList: Pick<DOMTokenList, "toggle">;
  style: Pick<CSSStyleDeclaration, "colorScheme">;
}

export interface AppearanceMediaQuery {
  matches: boolean;
  addEventListener(type: "change", listener: () => void): void;
  removeEventListener(type: "change", listener: () => void): void;
}

export function installSystemAppearance(
  root: AppearanceRoot,
  mediaQuery: AppearanceMediaQuery,
  forcedDark?: boolean,
): () => void {
  const apply = () => {
    const isDark = forcedDark ?? mediaQuery.matches;
    root.classList.toggle("dark", isDark);
    root.style.colorScheme = isDark ? "dark" : "light";
  };

  apply();
  if (forcedDark !== undefined) return () => {};

  mediaQuery.addEventListener("change", apply);
  return () => mediaQuery.removeEventListener("change", apply);
}
