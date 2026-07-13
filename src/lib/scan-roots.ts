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
