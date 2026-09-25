export type EntryKind = "directory" | "file" | "symlink" | "other";
export type ScanBackend = "jwalk" | "getattrlistbulk" | "mft" | "statx";
export type ScanPhase = "scanning" | "finishing";
export type SizeMetric = "allocated" | "logical";
export type CompressionCapabilityStatus =
  | "inspectOnly"
  | "unsupported"
  | "unavailable";

export interface CompressionCapability {
  status: CompressionCapabilityStatus;
  filesystem: string;
  volumeSupportsTransparentCompression: boolean;
  writerAvailable: boolean;
  algorithms: string[];
  detail: string;
}

export type CompressionStateKind =
  | "compressed"
  | "notCompressed"
  | "enabled"
  | "disabled"
  | "inherited"
  | "notApplicable"
  | "unsupported"
  | "unavailable";
export type CompressionStateScope = "existingData" | "futureWrites" | "none";

export interface CompressionState {
  state: CompressionStateKind;
  scope: CompressionStateScope;
  format: string | null;
  detail: string;
}

export type EstimateStatus =
  | "estimated"
  | "notCandidate"
  | "unsupported"
  | "unavailable"
  | "cancelled";
export type EstimateConfidence = "high" | "medium" | "low" | "none";
export type AlgorithmFidelity = "exact" | "proxy" | "none";

export interface SavingsEstimate {
  status: EstimateStatus;
  algorithm: string | null;
  fidelity: AlgorithmFidelity;
  confidence: EstimateConfidence;
  sampledBytes: number;
  logicalBytes: number;
  allocatedBytes: number;
  estimatedSavingsLower: number | null;
  estimatedSavingsUpper: number | null;
  estimatorVersion: number;
  detail: string;
}

export type CompressionOperation = "compress" | "decompress";
export type CompressionPlanStatus = "prepared" | "blocked";
export type CompressionPlanBlockerCode =
  | "writerUnavailable"
  | "capabilityUnavailable"
  | "stateUnavailable"
  | "alreadyInRequestedState"
  | "fileTooLarge";

export interface CompressionPlanBlocker {
  code: CompressionPlanBlockerCode;
  detail: string;
}

export interface CompressionPlanPreview {
  planId: number;
  scanId: number;
  nodeId: number;
  operation: CompressionOperation;
  status: CompressionPlanStatus;
  filesystem: string;
  algorithm: string | null;
  logicalBytes: number;
  allocatedBytes: number;
  allocatedSizeIsEstimate: boolean;
  estimatedReadBytes: number;
  estimatedWriteBytes: number;
  requiredFreeSpaceBytes: number | null;
  linkCount: number;
  warnings: string[];
  blockers: CompressionPlanBlocker[];
}

export type PlanValidationStatus = "valid" | "changed" | "unavailable";

export interface PlanValidation {
  planId: number;
  status: PlanValidationStatus;
  detail: string;
}

export interface ScanProgress {
  phase: ScanPhase;
  entriesScanned: number;
  filesScanned: number;
  directoriesScanned: number;
  logicalBytes: number;
  allocatedBytes: number;
  skippedEntries: number;
  currentPath: string;
  elapsedMs: number;
  largestItems: ScanItem[];
}

export interface ScanProgressPresentation {
  statusLabel: "Scanning" | "Finishing" | "Stopping";
  totalLabel: "Found so far" | "Space found";
  currentLabel: "Preparing results…" | null;
  announcement: string;
}

const COMPACT_COUNT_FORMATTER = new Intl.NumberFormat(undefined, {
  notation: "compact",
});

function formatEntryCount(count: number): string {
  return `${formatCount(count)} ${count === 1 ? "entry" : "entries"}`;
}

export function scanProgressPresentation(
  progress: Pick<ScanProgress, "phase" | "entriesScanned" | "allocatedBytes">,
  cancelling: boolean,
): ScanProgressPresentation {
  const finishing = progress.phase === "finishing";
  return {
    statusLabel: cancelling ? "Stopping" : finishing ? "Finishing" : "Scanning",
    totalLabel: finishing ? "Space found" : "Found so far",
    currentLabel: finishing ? "Preparing results…" : null,
    announcement: cancelling
      ? finishing
        ? `Stopping while preparing results for ${formatEntryCount(progress.entriesScanned)}.`
        : `Stopping after ${formatEntryCount(progress.entriesScanned)}.`
      : finishing
        ? `Finishing the scan after ${formatEntryCount(progress.entriesScanned)}.`
        : progress.entriesScanned === 0 && progress.allocatedBytes === 0
          ? ""
          : `Scanned ${formatEntryCount(progress.entriesScanned)} and ${formatBytes(progress.allocatedBytes)}.`,
  };
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
  suppressedItems: number;
  itemsTruncated: boolean;
  breadcrumbs: Breadcrumb[];
  items: ScanItem[];
  chartItems: ChartItem[];
}

export interface DirectorySearchResult {
  scanId: number;
  nodeId: number;
  query: string;
  metric: SizeMetric;
  totalMatches: number;
  itemsTruncated: boolean;
  items: ScanItem[];
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
  skippedCloudEntries: number;
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
  | { event: "progress"; scanId: number; progress: ScanProgress }
  | { event: "completed"; response: ScanResponse }
  | { event: "failed"; scanId: number; message: string };

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
  return COMPACT_COUNT_FORMATTER.format(count);
}

export function formatUnavailableItems(count: number): string {
  const normalized = Number.isFinite(count) ? Math.max(0, Math.floor(count)) : 0;
  return `${formatCount(normalized)} ${normalized === 1 ? "item was" : "items were"} unavailable during this scan`;
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
  if (backend === "getattrlistbulk") return "macOS native";
  if (backend === "mft") return "Windows native";
  if (backend === "statx") return "Linux native";
  return "Portable";
}

export function metricBytes(
  entry: Pick<ScanItem, "allocatedBytes" | "logicalBytes">,
  metric: SizeMetric,
): number {
  return metric === "logical" ? entry.logicalBytes : entry.allocatedBytes;
}

export function formatMetric(metric: SizeMetric): string {
  return metric === "logical" ? "Logical size" : "Space on disk";
}

export function formatCompressionState(state: CompressionState): string {
  switch (state.state) {
    case "compressed":
      return state.format ? `Compressed · ${state.format}` : "Compressed";
    case "notCompressed":
      return "Not compressed";
    case "enabled":
      return "Enabled for future writes";
    case "disabled":
      return "Disabled for future writes";
    case "inherited":
      return "Following filesystem policy";
    case "notApplicable":
      return "Not applicable";
    case "unsupported":
      return "Unsupported on this filesystem";
    case "unavailable":
      return "Couldn’t be checked";
  }
}

export function formatSavingsEstimate(estimate: SavingsEstimate): string {
  if (
    estimate.status !== "estimated" ||
    estimate.estimatedSavingsLower === null ||
    estimate.estimatedSavingsUpper === null
  ) {
    switch (estimate.status) {
      case "notCandidate":
        return "No estimate needed";
      case "unsupported":
        return "Not supported";
      case "unavailable":
        return "Couldn’t be estimated";
      case "cancelled":
        return "Estimate cancelled";
      case "estimated":
        return "Couldn’t be estimated";
    }
  }
  if (estimate.estimatedSavingsUpper === 0) return "No likely savings";
  if (estimate.estimatedSavingsLower === estimate.estimatedSavingsUpper) {
    return formatBytes(estimate.estimatedSavingsUpper);
  }
  return `${formatBytes(estimate.estimatedSavingsLower)}–${formatBytes(estimate.estimatedSavingsUpper)}`;
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
