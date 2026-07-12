export type EntryKind = "directory" | "file" | "symlink" | "other";
export type ScanBackend = "jwalk" | "getattrlistbulk";

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
  id: number;
  name: string;
  kind: EntryKind;
  logicalBytes: number;
  allocatedBytes: number;
  fileCount: number;
  directoryCount: number;
}

export interface Breadcrumb {
  id: number;
  name: string;
}

export interface ChartItem {
  id: number | null;
  name: string;
  kind: EntryKind;
  logicalBytes: number;
  allocatedBytes: number;
  children: ChartItem[];
}

export interface DirectoryView {
  scanId: number;
  nodeId: number;
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
  backend: ScanBackend;
  logicalBytes: number;
  allocatedBytes: number;
  fileCount: number;
  directoryCount: number;
  skippedEntries: number;
  skippedFilesystems: number;
  duplicateHardLinks: number;
  traversalUs: number;
  aggregationUs: number;
  indexingUs: number;
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

  const totalSeconds = Math.round(milliseconds / 1_000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}m ${seconds}s`;
}

export function formatPercent(part: number, total: number): string {
  if (!Number.isFinite(part) || !Number.isFinite(total) || total <= 0) {
    return "0.0%";
  }
  return `${Math.max(0, Math.min(100, (part / total) * 100)).toFixed(1)}%`;
}

export function formatBackend(backend: ScanBackend): string {
  return backend === "getattrlistbulk" ? "macOS native" : "Portable";
}

export function isCancellationError(error: unknown): boolean {
  return String(error).toLowerCase().includes("cancelled");
}

export function describeEntry(entry: Pick<ScanItem, "kind" | "fileCount" | "directoryCount">): string {
  switch (entry.kind) {
    case "directory":
      return `${formatCount(entry.fileCount)} files · ${formatCount(entry.directoryCount)} folders`;
    case "file":
      return "File";
    case "symlink":
      return "Symbolic link · not followed";
    case "other":
      return "Other filesystem entry";
  }
}
