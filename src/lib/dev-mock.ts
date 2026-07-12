import { mockIPC } from "@tauri-apps/api/mocks";
import type {
  ChartItem,
  DirectoryView,
  ScanProgress,
  ScanResponse,
  SizeMetric,
} from "./scanner";

type DevScenario =
  | "complete"
  | "scanning"
  | "error"
  | "navigation-error"
  | "reveal-error";

const ROOT = "/Users/demo";
const scanId = 42;

export function installDevMock(requestedScenario: string) {
  const scenario: DevScenario = isScenario(requestedScenario)
    ? requestedScenario
    : "complete";
  let rejectPendingScan: ((reason: string) => void) | null = null;

  mockIPC(async (command, payload = {}) => {
    const args = payload as unknown as Record<string, unknown>;
    switch (command) {
      case "plugin:dialog|open":
        return ROOT;
      case "scan_directory": {
        const channelId = (args.onEvent as { id: number }).id;
        emitChannel(channelId, 0, { event: "started", scanId, root: ROOT });
        await delay(35);
        emitChannel(channelId, 1, {
          event: "progress",
          scanId,
          progress: mockProgress(),
        });

        if (scenario === "error") {
          throw "Permission denied while reading the selected folder.";
        }
        if (scenario === "scanning") {
          return new Promise<ScanResponse>((_, reject) => {
            rejectPendingScan = reject;
          });
        }
        await delay(75);
        return mockResponse();
      }
      case "cancel_scan":
        rejectPendingScan?.("Scan cancelled.");
        rejectPendingScan = null;
        return true;
      case "open_scan_directory":
        if (scenario === "navigation-error") {
          throw "The mocked snapshot is no longer available.";
        }
        return metricView(
          mockDirectoryView(Number(args.nodeId)),
          args.metric === "logical" ? "logical" : "allocated",
        );
      case "reveal_scan_item":
        if (scenario === "reveal-error") {
          throw "The mocked item disappeared after the scan completed.";
        }
        return null;
      default:
        throw new Error(`Unhandled development mock command: ${command}`);
    }
  });
}

function isScenario(value: string): value is DevScenario {
  return [
    "complete",
    "scanning",
    "error",
    "navigation-error",
    "reveal-error",
  ].includes(value);
}

function emitChannel(channelId: number, index: number, message: unknown) {
  const internals = (
    window as unknown as Window & {
      __TAURI_INTERNALS__: { runCallback(id: number, payload: unknown): void };
    }
  ).__TAURI_INTERNALS__;
  internals.runCallback(channelId, { index, message });
}

function mockProgress(): ScanProgress {
  return {
    entriesScanned: 18_432,
    filesScanned: 17_906,
    directoriesScanned: 526,
    logicalBytes: 318_901_321_728,
    allocatedBytes: 302_795_292_672,
    skippedEntries: 2,
    currentPath: `${ROOT}/Library/Application Support/Design Archive`,
    elapsedMs: 428,
    largestItems: rootView().items
      .filter((item) => item.kind === "file")
      .map((item) => ({
        ...item,
        logicalBytes: Math.round(item.logicalBytes * 0.58),
        allocatedBytes: Math.round(item.allocatedBytes * 0.58),
      })),
  };
}

function mockResponse(): ScanResponse {
  return {
    scanId,
    result: {
      root: ROOT,
      displayName: "demo",
      backend: "getattrlistbulk",
      logicalBytes: 526_133_493_760,
      allocatedBytes: 501_437_087_744,
      fileCount: 128_492,
      directoryCount: 8_731,
      skippedEntries: 2,
      skippedFilesystems: 1,
      duplicateHardLinks: 14_218,
      traversalUs: 831_420,
      aggregationUs: 11_203,
      indexingUs: 0,
      elapsedMs: 843,
      allocatedSizeIsEstimate: false,
      hardLinkDeduplicationSupported: true,
      sameFilesystemEnforced: true,
    },
    view: metricView(rootView(), "allocated"),
  };
}

function metricView(view: DirectoryView, metric: SizeMetric): DirectoryView {
  const bytes =
    metric === "logical"
      ? (item: { logicalBytes: number }) => item.logicalBytes
      : (item: { allocatedBytes: number }) => item.allocatedBytes;
  const compare = (
    left: { name: string; logicalBytes: number; allocatedBytes: number },
    right: { name: string; logicalBytes: number; allocatedBytes: number },
  ) => bytes(right) - bytes(left) || left.name.localeCompare(right.name);
  const rankChart = (items: ChartItem[]): ChartItem[] =>
    items
      .map((item) => ({ ...item, children: rankChart(item.children) }))
      .sort(compare);

  return {
    ...view,
    items: [...view.items].sort(compare),
    chartItems: rankChart(view.chartItems),
  };
}

function rootView(): DirectoryView {
  return {
    scanId,
    nodeId: 0,
    root: ROOT,
    path: ROOT,
    displayName: "demo",
    logicalBytes: 526_133_493_760,
    allocatedBytes: 501_437_087_744,
    totalItems: 4,
    itemsTruncated: false,
    breadcrumbs: [{ id: 0, name: "demo" }],
    items: [
      {
        id: 1,
        name: "Library",
        kind: "directory",
        logicalBytes: 251_255_586_816,
        allocatedBytes: 236_223_201_280,
        fileCount: 84_210,
        directoryCount: 6_420,
      },
      {
        id: 2,
        name: "Pictures",
        kind: "directory",
        logicalBytes: 198_642_237_440,
        allocatedBytes: 193_273_528_320,
        fileCount: 31_440,
        directoryCount: 1_240,
      },
      {
        id: 3,
        name: "archive-2024.zip",
        kind: "file",
        logicalBytes: 71_940_358_144,
        allocatedBytes: 71_940_358_144,
        fileCount: 1,
        directoryCount: 0,
      },
      {
        id: 4,
        name: "Latest project",
        kind: "symlink",
        logicalBytes: 0,
        allocatedBytes: 0,
        fileCount: 0,
        directoryCount: 0,
      },
    ],
    chartItems: [
      {
        id: 1,
        name: "Library",
        kind: "directory",
        logicalBytes: 251_255_586_816,
        allocatedBytes: 236_223_201_280,
        children: [
          {
            id: 5,
            name: "Caches",
            kind: "directory",
            logicalBytes: 139_586_437_120,
            allocatedBytes: 128_849_018_880,
            children: [],
          },
          {
            id: 6,
            name: "Application Support",
            kind: "directory",
            logicalBytes: 96_636_764_160,
            allocatedBytes: 91_268_055_040,
            children: [],
          },
        ],
      },
      {
        id: 2,
        name: "Pictures",
        kind: "directory",
        logicalBytes: 198_642_237_440,
        allocatedBytes: 193_273_528_320,
        children: [],
      },
      {
        id: 3,
        name: "archive-2024.zip",
        kind: "file",
        logicalBytes: 71_940_358_144,
        allocatedBytes: 71_940_358_144,
        children: [],
      },
    ],
  };
}

function mockDirectoryView(nodeId: number): DirectoryView {
  if (nodeId === 0) return rootView();
  if (nodeId === 2) {
    return emptyDirectoryView(2, "Pictures", 198_642_237_440, 193_273_528_320, [
      { id: 0, name: "demo" },
      { id: 2, name: "Pictures" },
    ]);
  }
  if (nodeId === 5 || nodeId === 6) {
    const name = nodeId === 5 ? "Caches" : "Application Support";
    const logicalBytes = nodeId === 5 ? 139_586_437_120 : 96_636_764_160;
    const allocatedBytes = nodeId === 5 ? 128_849_018_880 : 91_268_055_040;
    return emptyDirectoryView(nodeId, name, logicalBytes, allocatedBytes, [
      { id: 0, name: "demo" },
      { id: 1, name: "Library" },
      { id: nodeId, name },
    ]);
  }
  if (nodeId !== 1) {
    throw new Error(`Unknown mocked node: ${nodeId}`);
  }
  return {
    scanId,
    nodeId,
    root: ROOT,
    path: `${ROOT}/Library`,
    displayName: "Library",
    logicalBytes: 251_255_586_816,
    allocatedBytes: 236_223_201_280,
    totalItems: 2,
    itemsTruncated: false,
    breadcrumbs: [
      { id: 0, name: "demo" },
      { id: 1, name: "Library" },
    ],
    items: [
      {
        id: 5,
        name: "Caches",
        kind: "directory",
        logicalBytes: 139_586_437_120,
        allocatedBytes: 128_849_018_880,
        fileCount: 42_130,
        directoryCount: 2_910,
      },
      {
        id: 6,
        name: "Application Support",
        kind: "directory",
        logicalBytes: 96_636_764_160,
        allocatedBytes: 91_268_055_040,
        fileCount: 35_118,
        directoryCount: 2_604,
      },
    ],
    chartItems: [
      {
        id: 5,
        name: "Caches",
        kind: "directory",
        logicalBytes: 139_586_437_120,
        allocatedBytes: 128_849_018_880,
        children: [],
      },
      {
        id: 6,
        name: "Application Support",
        kind: "directory",
        logicalBytes: 96_636_764_160,
        allocatedBytes: 91_268_055_040,
        children: [],
      },
    ],
  };
}

function emptyDirectoryView(
  nodeId: number,
  name: string,
  logicalBytes: number,
  allocatedBytes: number,
  breadcrumbs: DirectoryView["breadcrumbs"],
): DirectoryView {
  return {
    scanId,
    nodeId,
    root: ROOT,
    path: `${ROOT}/${breadcrumbs
      .slice(1)
      .map((breadcrumb) => breadcrumb.name)
      .join("/")}`,
    displayName: name,
    logicalBytes,
    allocatedBytes,
    totalItems: 0,
    itemsTruncated: false,
    breadcrumbs,
    items: [],
    chartItems: [],
  };
}

function delay(milliseconds: number) {
  return new Promise((resolve) => window.setTimeout(resolve, milliseconds));
}
