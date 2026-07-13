const LIST_NAVIGATION_KEYS = new Set([
  "ArrowUp",
  "ArrowDown",
  "Home",
  "End",
]);

export function listNavigationTarget(
  ids: readonly number[],
  currentId: number | null,
  key: string,
): number | null {
  if (!LIST_NAVIGATION_KEYS.has(key) || ids.length === 0) return null;
  if (key === "Home") return ids[0];
  if (key === "End") return ids.at(-1) ?? null;

  const currentIndex = currentId === null ? -1 : ids.indexOf(currentId);
  if (currentIndex < 0) return ids[0];
  if (key === "ArrowDown") {
    return ids[Math.min(currentIndex + 1, ids.length - 1)];
  }

  return ids[Math.max(currentIndex - 1, 0)];
}
