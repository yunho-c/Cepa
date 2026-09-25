export interface ExplorerSplit {
  stacked: boolean;
  available: number;
  min: number;
  max: number;
  defaultWidth: number;
  width: number;
}

/** Pixel bounds keep both panes usable while the window or divider moves. */
export function explorerSplit(
  containerWidth: number,
  viewportWidth: number,
  preferredWidth: number | null = null,
): ExplorerSplit {
  const stacked = viewportWidth < 560;
  const available = Math.max(0, containerWidth - (stacked ? 0 : 1));
  const compact = viewportWidth <= 720;
  const max = Math.max(0, Math.min(1040, available - (compact ? 300 : 320)));
  const min = Math.min(compact ? 196 : 320, max);
  const clamp = (width: number) => Math.min(max, Math.max(min, width));
  const defaultWidth = clamp(compact ? available * 0.36 : Math.max(340, viewportWidth * 0.40));
  return {
    stacked,
    available,
    min,
    max,
    defaultWidth,
    width: preferredWidth === null ? defaultWidth : clamp(preferredWidth),
  };
}

export function explorerSplitKey(
  key: string,
  split: ExplorerSplit,
  largeStep: boolean,
): number | null | undefined {
  const step = largeStep ? 40 : 10;
  switch (key) {
    case "ArrowLeft": return Math.max(split.min, split.width - step);
    case "ArrowRight": return Math.min(split.max, split.width + step);
    case "Home": return split.min;
    case "End": return split.max;
    case "Enter": return null;
    default: return undefined;
  }
}
