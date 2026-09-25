import type { ChartItem, DirectoryView, ScanItem } from "./scanner";

const MIB = 1024 ** 2;
export const STRESS_LIST_ITEMS = 500;
export const STRESS_CHART_ITEMS = 512;

export function createStressView(scanId: number, root: string): DirectoryView {
  const directories = Array.from({ length: 16 }, (_, index) =>
    stressDirectory(index + 1),
  );
  const files = Array.from({ length: STRESS_LIST_ITEMS - directories.length }, (_, index) => {
    const bytes = MIB - (index * 1024);
    return {
      id: 10_000 + index,
      name: `capture-${String(index + 1).padStart(4, "0")}.mov`,
      kind: "file",
      logicalBytes: bytes,
      allocatedBytes: bytes,
      fileCount: 1,
      directoryCount: 0,
    } satisfies ScanItem;
  });
  const items = [...directories, ...files];
  const fileBytes = files.reduce((total, item) => total + item.allocatedBytes, 0);
  const directoryBytes = directories.reduce(
    (total, item) => total + item.allocatedBytes,
    0,
  );
  const chartItems = stressChart(directoryBytes, fileBytes);

  return {
    scanId,
    nodeId: 0,
    root,
    path: root,
    displayName: "Stress fixture",
    logicalBytes: directoryBytes + fileBytes,
    allocatedBytes: directoryBytes + fileBytes,
    totalItems: items.length,
    suppressedItems: 0,
    itemsTruncated: false,
    breadcrumbs: [{ id: 0, name: "Stress fixture" }],
    items,
    chartItems,
  };
}

export function countChartItems(items: ChartItem[]): number {
  return items.reduce(
    (total, item) => total + 1 + countChartItems(item.children),
    0,
  );
}

export function createStressDirectoryView(
  rootView: DirectoryView,
  nodeId: number,
): DirectoryView {
  if (nodeId === rootView.nodeId) return rootView;
  const path = findChartPath(rootView.chartItems, nodeId);
  const item = path?.at(-1);
  if (!path || !item || item.id === null || item.kind !== "directory") {
    throw new Error(`Unknown stress-fixture folder: ${nodeId}`);
  }
  const items = item.children
    .filter((child): child is ChartItem & { id: number } => child.id !== null)
    .map((child) => ({
      id: child.id,
      name: child.name,
      kind: child.kind,
      logicalBytes: child.logicalBytes,
      allocatedBytes: child.allocatedBytes,
      fileCount: child.kind === "file" ? 1 : child.children.length,
      directoryCount: child.kind === "directory" ? 1 : 0,
    } satisfies ScanItem));

  return {
    ...rootView,
    nodeId: item.id,
    path: `${rootView.root}/${path.map((entry) => entry.name).join("/")}`,
    displayName: item.name,
    logicalBytes: item.logicalBytes,
    allocatedBytes: item.allocatedBytes,
    totalItems: items.length,
    suppressedItems: 0,
    itemsTruncated: false,
    breadcrumbs: [
      { id: rootView.nodeId, name: rootView.displayName },
      ...path.map((entry) => ({ id: entry.id ?? rootView.nodeId, name: entry.name })),
    ],
    items,
    chartItems: item.children,
  };
}

function stressDirectory(id: number): ScanItem {
  const bytes = 256 * MIB;
  return {
    id,
    name: `Project ${String(id).padStart(2, "0")}`,
    kind: "directory",
    logicalBytes: bytes,
    allocatedBytes: bytes,
    fileCount: 16,
    directoryCount: 16,
  };
}

function findChartPath(
  items: ChartItem[],
  nodeId: number,
  ancestors: ChartItem[] = [],
): ChartItem[] | null {
  for (const item of items) {
    if (item.id === null) continue;
    const path = [...ancestors, item];
    if (item.id === nodeId) return path;
    const nested = findChartPath(item.children, nodeId, path);
    if (nested) return nested;
  }
  return null;
}

function stressChart(directoryBytes: number, fileBytes: number): ChartItem[] {
  let remainingGrandchildren = STRESS_CHART_ITEMS - 17 - (16 * 16);
  let nextId = 20_000;
  const directories = Array.from({ length: 16 }, (_, outer) => {
    const children = Array.from({ length: 16 }, (_, inner) => {
      const bytes = 16 * MIB;
      const childId = nextId++;
      const grandchildren = remainingGrandchildren > 0
        ? [
            {
              id: nextId++,
              name: `Asset ${outer + 1}-${inner + 1}`,
              kind: "file",
              logicalBytes: bytes,
              allocatedBytes: bytes,
              children: [],
            } satisfies ChartItem,
          ]
        : [];
      if (grandchildren.length > 0) remainingGrandchildren -= 1;
      return {
        id: childId,
        name: `Scene ${outer + 1}-${inner + 1}`,
        kind: "directory",
        logicalBytes: bytes,
        allocatedBytes: bytes,
        children: grandchildren,
      } satisfies ChartItem;
    });
    return {
      id: outer + 1,
      name: `Project ${String(outer + 1).padStart(2, "0")}`,
      kind: "directory",
      logicalBytes: directoryBytes / 16,
      allocatedBytes: directoryBytes / 16,
      children,
    } satisfies ChartItem;
  });

  return [
    ...directories,
    {
      id: null,
      name: `${STRESS_LIST_ITEMS - 16} more items`,
      kind: "other",
      logicalBytes: fileBytes,
      allocatedBytes: fileBytes,
      children: [],
    },
  ];
}
