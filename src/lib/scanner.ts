export type EntryKind = "directory" | "file" | "symlink" | "other";

export interface ScanProgress {
  entriesScanned: number;
  filesScanned: number;
  directoriesScanned: number;
  logicalBytes: number;
  allocatedBytes: number;
  skippedEntries: number;
  currentPath: string;
  elapsedMs: number;
}

export interface ScanItem {
  name: string;
  path: string;
  kind: EntryKind;
  logicalBytes: number;
  allocatedBytes: number;
  fileCount: number;
  directoryCount: number;
}

export interface Breadcrumb {
  name: string;
  path: string;
}

export interface ChartItem {
  name: string;
  path: string | null;
  kind: EntryKind;
  logicalBytes: number;
  allocatedBytes: number;
  children: ChartItem[];
}

export interface DirectoryView {
  scanId: number;
  root: string;
  path: string;
  displayName: string;
  logicalBytes: number;
  allocatedBytes: number;
  totalItems: number;
  itemsTruncated: boolean;
  breadcrumbs: Breadcrumb[];
  items: ScanItem[];
  chartItems: ChartItem[];
}

export interface ScanResult {
  root: string;
  displayName: string;
  backend: "jwalk";
  logicalBytes: number;
  allocatedBytes: number;
  fileCount: number;
  directoryCount: number;
  skippedEntries: number;
  skippedFilesystems: number;
  duplicateHardLinks: number;
  traversalMs: number;
  aggregationMs: number;
  indexingMs: number;
  elapsedMs: number;
  allocatedSizeIsEstimate: boolean;
  hardLinkDeduplicationSupported: boolean;
  sameFilesystemEnforced: boolean;
}

export interface ScanResponse {
  scanId: number;
  result: ScanResult;
  view: DirectoryView;
}

export type ScanEvent =
  | { event: "started"; scanId: number; root: string }
  | { event: "progress"; scanId: number; progress: ScanProgress };

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";

  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  const unitIndex = Math.min(
    Math.floor(Math.log(bytes) / Math.log(1024)),
    units.length - 1,
  );
  const value = bytes / 1024 ** unitIndex;
  const digits = value >= 100 || unitIndex === 0 ? 0 : value >= 10 ? 1 : 2;

  return `${value.toFixed(digits)} ${units[unitIndex]}`;
}

export function formatCount(count: number): string {
  return new Intl.NumberFormat(undefined, { notation: "compact" }).format(count);
}

export function formatDuration(milliseconds: number): string {
  if (milliseconds < 1_000) return `${Math.max(milliseconds, 0)} ms`;
  if (milliseconds < 60_000) return `${(milliseconds / 1_000).toFixed(1)} s`;

  const minutes = Math.floor(milliseconds / 60_000);
  const seconds = Math.round((milliseconds % 60_000) / 1_000);
  return `${minutes}m ${seconds}s`;
}
