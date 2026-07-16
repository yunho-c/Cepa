export interface ScanRoot {
  name: string;
  path: string;
  displayPath: string;
  totalBytes: number;
  availableBytes: number;
  isRemovable: boolean;
  isReadOnly: boolean;
}

export type ScanRootsStatus = "idle" | "loading" | "ready" | "error";

export type ScanRootsPreview = Exclude<ScanRootsStatus, "idle">;
export type ScanRootsRetryFocus = "loading" | "root" | "retry" | "fallback";

export function scanRootsPreviewStatus(value: string | null): ScanRootsPreview | null {
  switch (value) {
    case "preview":
    case "ready":
      return "ready";
    case "loading":
    case "error":
      return value;
    default:
      return null;
  }
}

export function scanRootsRetryFocusTarget(
  status: ScanRootsStatus,
  rootCount: number,
): ScanRootsRetryFocus {
  if (status === "loading") return "loading";
  if (rootCount > 0) return "root";
  if (status === "error") return "retry";
  return "fallback";
}

export function shouldShowScanRoots(
  status: ScanRootsStatus,
  rootCount: number,
): boolean {
  return status === "loading" || status === "error" || rootCount > 0;
}

export function scanRootUsedBytes(root: ScanRoot): number {
  return Math.max(0, root.totalBytes - root.availableBytes);
}

export function scanRootUsedPercent(root: ScanRoot): number {
  if (!Number.isFinite(root.totalBytes) || root.totalBytes <= 0) return 0;
  return Math.max(
    0,
    Math.min(100, (scanRootUsedBytes(root) / root.totalBytes) * 100),
  );
}
