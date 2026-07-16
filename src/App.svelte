<script lang="ts">
  import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
  import { getCurrentWindow, type DragDropEvent } from "@tauri-apps/api/window";
  import { open } from "@tauri-apps/plugin-dialog";
  import { onMount, tick } from "svelte";
  import {
    AlertCircle,
    ArrowLeft,
    ChevronRight,
    CircleStop,
    File,
    Folder,
    FolderDown,
    FolderOpen,
    FolderSearch,
    Link2,
    RefreshCw,
    Search,
    ScanSearch,
    X,
  } from "@lucide/svelte";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import CepaMark from "$lib/components/cepa-mark.svelte";
  import ScanRootPicker from "$lib/components/scan-root-picker.svelte";
  import { isCurrentCompletedScanRequest } from "$lib/completed-scan-request";
  import { droppedItemName, folderDropAction } from "$lib/folder-drop";
  import { listNavigationTarget } from "$lib/list-navigation";
  import {
    shouldShowScanRoots,
    type ScanRoot,
    type ScanRootsStatus,
  } from "$lib/scan-roots";
  import {
    formatBytes,
    formatBackend,
    formatCompressionState,
    formatCount,
    formatDuration,
    formatMetric,
    formatPercent,
    formatSavingsEstimate,
    formatUnavailableItems,
    describeEntry,
    isCancellationError,
    metricBytes,
    scanProgressPresentation,
    type ChartItem,
    type CompressionCapability,
    type CompressionState,
    type DirectoryView,
    type DirectorySearchResult,
    type ScanEvent,
    type ScanItem,
    type ScanProgress,
    type ScanResponse,
    type ScanResult,
    type SavingsEstimate,
    type SizeMetric,
  } from "$lib/scanner";
  import {
    createDesktopMenuAvailabilitySync,
    desktopCommandForMenu,
    desktopCommandForKeydown,
    desktopMenuAvailability,
    primaryModifierForPlatform,
    scanPathPlaceholderForPlatform,
    type DesktopCommand,
    type DesktopCommandContext,
    type DesktopMenuAvailability,
  } from "$lib/shortcuts";
  import { createSunburst, sunburstNavigationTarget } from "$lib/sunburst";

  type Status = "idle" | "scanning" | "cancelling" | "cancelled" | "complete" | "error";

  let path = $state("");
  let status = $state<Status>("idle");
  let scanId = $state<number | null>(null);
  let progress = $state<ScanProgress | null>(null);
  let result = $state<ScanResult | null>(null);
  let view = $state<DirectoryView | null>(null);
  let selectedEntry = $state<ChartItem | ScanItem | null>(null);
  let inspectedEntry = $state<ChartItem | ScanItem | null>(null);
  let isNavigating = $state(false);
  let navigationSequence = 0;
  let isDiscardingScan = $state(false);
  let errorHeading = $state("That scan didn’t start.");
  let errorMessage = $state("");
  let scanStarted = $state(false);
  let scanActionError = $state("");
  let navigationError = $state("");
  let navigationErrorTitle = $state("That folder could not be opened.");
  let revealError = $state("");
  let revealingNodeId = $state<number | null>(null);
  let revealSequence = 0;
  let sizeMetric = $state<SizeMetric>("allocated");
  let compressionCapability = $state<CompressionCapability | null>(null);
  let compressionState = $state<CompressionState | null>(null);
  let isInspectingCompression = $state(false);
  let inspectionSequence = 0;
  let savingsEstimate = $state<SavingsEstimate | null>(null);
  let isEstimatingSavings = $state(false);
  let isCancellingEstimate = $state(false);
  let estimateActionError = $state("");
  let activeEstimateRequestId = $state<number | null>(null);
  let estimateRequestSequence = 0;
  let estimateActionButton: HTMLButtonElement | null = $state(null);
  let estimateCancelButton: HTMLButtonElement | null = $state(null);
  let estimateActionNotice: HTMLDivElement | undefined = $state();
  let inspectionReturnTarget: (HTMLElement | SVGGElement) | null = null;
  let resultHeading: HTMLHeadingElement | undefined = $state();
  let landingHeading: HTMLHeadingElement | undefined = $state();
  let viewHeading: HTMLHeadingElement | undefined = $state();
  let sunburstElement: SVGSVGElement | undefined = $state();
  let chartFocusId: number | null = $state(null);
  let itemListElement: HTMLDivElement | undefined = $state();
  let listFocusId: number | null = $state(null);
  let stateNotice: HTMLDivElement | undefined = $state();
  let scanActionNotice: HTMLDivElement | undefined = $state();
  let navigationNotice: HTMLDivElement | undefined = $state();
  let revealNotice: HTMLDivElement | undefined = $state();
  let searchOpen = $state(false);
  let searchQuery = $state("");
  let searchResult = $state<DirectorySearchResult | null>(null);
  let isSearching = $state(false);
  let searchError = $state("");
  let searchInput: HTMLInputElement | null = $state(null);
  let searchToggleButton: HTMLButtonElement | null = $state(null);
  let searchSequence = 0;
  let activeSearchRequestId: number | null = null;
  let searchTimer: ReturnType<typeof setTimeout> | null = null;
  const dropPreview =
    import.meta.env.DEV &&
    new URLSearchParams(window.location.search).get("drop") === "active";
  const hasDevMock =
    import.meta.env.DEV &&
    new URLSearchParams(window.location.search).has("mock");
  const rootsPreview =
    import.meta.env.DEV &&
    new URLSearchParams(window.location.search).get("roots") === "preview";
  let dropActive = $state(dropPreview);
  let droppedPaths = $state<string[]>(
    dropPreview ? ["/Users/demo/Design Archive"] : [],
  );
  let preparingScanRoot = $state<string | null>(null);
  let scanRoots = $state<ScanRoot[]>(
    rootsPreview
      ? [
          {
            name: "Macintosh HD",
            path: "/System/Volumes/Data",
            displayPath: "/",
            totalBytes: 1_000_000_000_000,
            availableBytes: 286_000_000_000,
            isRemovable: false,
            isReadOnly: false,
          },
          {
            name: "Archive",
            path: "/Volumes/Archive",
            displayPath: "/Volumes/Archive",
            totalBytes: 2_000_000_000_000,
            availableBytes: 1_240_000_000_000,
            isRemovable: true,
            isReadOnly: false,
          },
        ]
      : [],
  );
  let scanRootsStatus = $state<ScanRootsStatus>(
    rootsPreview ? "ready" : "idle",
  );
  let dropSequence = 0;

  const primaryModifier = primaryModifierForPlatform(navigator.platform);
  const scanPathPlaceholder = scanPathPlaceholderForPlatform(navigator.platform);
  const syncDesktopMenuAvailability = createDesktopMenuAvailabilitySync(
    (availability) => invoke("set_desktop_menu_availability", { availability }),
  );
  const primaryShortcutLabel = primaryModifier === "meta" ? "⌘" : "Ctrl+";
  const backShortcutLabel = primaryModifier === "meta" ? "⌥←" : "Alt+←";

  const isBusy = $derived(
    status === "scanning" ||
      status === "cancelling" ||
      preparingScanRoot !== null ||
      isDiscardingScan,
  );
  const isResultBusy = $derived(isNavigating || isDiscardingScan);
  const isPreparingDroppedFolder = $derived(
    preparingScanRoot !== null && droppedPaths.length > 0,
  );
  const dropOverlayVisible = $derived(dropActive || isPreparingDroppedFolder);
  const displayProgress = $derived(
    progress ?? ({
      phase: "scanning",
      entriesScanned: 0,
      filesScanned: 0,
      directoriesScanned: 0,
      logicalBytes: 0,
      allocatedBytes: 0,
      skippedEntries: 0,
      currentPath: path,
      elapsedMs: 0,
      largestItems: [],
    } satisfies ScanProgress),
  );
  const progressPresentation = $derived(
    scanProgressPresentation(displayProgress, status === "cancelling"),
  );
  const sunburstSegments = $derived(createSunburst(view?.chartItems ?? [], sizeMetric));
  $effect(() => {
    const interactiveIds = sunburstSegments.flatMap((segment) =>
      segment.item.id === null ? [] : [segment.item.id],
    );
    if (!interactiveIds.includes(chartFocusId ?? -1)) {
      chartFocusId = interactiveIds[0] ?? null;
    }
  });
  // Hover and keyboard focus should always coordinate the chart and list. A
  // clicked inspection remains the fallback when there is no transient target.
  const activeEntry = $derived(selectedEntry ?? inspectedEntry);
  const viewBytes = $derived(view ? metricBytes(view, sizeMetric) : 0);
  const parentId = $derived(view?.breadcrumbs.at(-2)?.id ?? null);
  const canEstimateSavings = $derived(
    inspectedEntry?.kind === "file" &&
      compressionState !== null &&
      ["notCompressed", "enabled", "disabled", "inherited"].includes(
        compressionState.state,
      ),
  );
  const scanTargetName = $derived(
    path.split(/[\\/]/).filter(Boolean).at(-1) ?? path,
  );
  const searchActive = $derived(searchQuery.trim().length > 0);
  const visibleItems = $derived(
    searchActive && searchResult ? searchResult.items : (view?.items ?? []),
  );
  const visibleItemIds = $derived(visibleItems.map((item) => item.id));
  $effect(() => {
    if (!visibleItemIds.includes(listFocusId ?? -1)) {
      listFocusId = visibleItemIds[0] ?? null;
    }
  });
  const directoryCountLabel = $derived(
    searchActive && searchError
      ? ""
      : isSearching
      ? "Searching…"
      : searchActive && searchResult
        ? searchResult.itemsTruncated
          ? `Top ${searchResult.items.length} of ${searchResult.totalMatches} matches`
          : `${searchResult.totalMatches} ${searchResult.totalMatches === 1 ? "match" : "matches"}`
        : view
          ? view.itemsTruncated
            ? `Top ${view.items.length} of ${view.totalItems}`
            : `${view.totalItems} items`
          : "",
  );
  const droppedFolderLabel = $derived(
    droppedPaths.length === 1
      ? droppedItemName(droppedPaths[0])
      : `${droppedPaths.length} items`,
  );
  const showsScanRoots = $derived(
    shouldShowScanRoots(scanRootsStatus, scanRoots.length),
  );

  async function handleListNavigation(
    event: KeyboardEvent,
    itemId: number,
    action: "open" | "reveal",
  ) {
    const targetId = listNavigationTarget(visibleItemIds, itemId, event.key);
    if (targetId === null) return;

    event.preventDefault();
    listFocusId = targetId;
    await tick();
    const actionTarget = itemListElement?.querySelector<HTMLElement>(
      `[data-list-${action}-id="${targetId}"]`,
    );
    const rowTarget = itemListElement?.querySelector<HTMLElement>(
      `[data-list-open-id="${targetId}"]`,
    );
    (actionTarget ?? rowTarget)?.focus();
  }

  function clearDropState() {
    dropActive = false;
    droppedPaths = [];
  }

  async function showScanEntryError(title: string, message: string) {
    if (status === "complete" && result && view) {
      navigationErrorTitle = title;
      navigationError = message;
      await tick();
      navigationNotice?.focus();
    } else {
      status = "error";
      scanStarted = false;
      scanActionError = "";
      errorHeading = title;
      errorMessage = message;
      await tick();
      stateNotice?.focus();
    }
  }

  async function startValidatedRoot(requestedPath: string, fromDrop: boolean) {
    const request = ++dropSequence;
    if (fromDrop) {
      dropActive = false;
      droppedPaths = [requestedPath];
    } else {
      clearDropState();
    }
    preparingScanRoot = requestedPath;
    try {
      const validatedPath = await invoke<string>("validate_scan_root", {
        path: requestedPath,
      });
      if (request !== dropSequence) return;
      preparingScanRoot = null;
      path = validatedPath;
      await startScan();
    } catch (error) {
      if (request !== dropSequence) return;
      preparingScanRoot = null;
      clearDropState();
      await showScanEntryError("That folder can’t be scanned.", String(error));
    }
  }

  async function loadScanRoots() {
    if (!isTauri() || hasDevMock || rootsPreview) return;
    scanRootsStatus = "loading";
    try {
      scanRoots = await invoke<ScanRoot[]>("list_scan_roots");
      scanRootsStatus = "ready";
    } catch {
      scanRoots = [];
      scanRootsStatus = "error";
    }
  }

  function handleNativeFolderDrop(event: { payload: DragDropEvent }) {
    const action = folderDropAction(event.payload, isBusy);
    switch (action.kind) {
      case "activate":
        droppedPaths = action.paths;
        dropActive = true;
        break;
      case "deactivate":
        clearDropState();
        break;
      case "scan":
        void startValidatedRoot(action.path, true);
        break;
      case "reject":
        clearDropState();
        void showScanEntryError(
          "Drop one folder at a time.",
          "Choose a single folder and try again.",
        );
        break;
      case "ignore":
        break;
    }
  }

  $effect(() => {
    const availability = desktopMenuAvailability(currentDesktopMenuAvailability());
    if (!isTauri() || hasDevMock) return;
    void syncDesktopMenuAvailability(availability);
  });

  onMount(() => {
    void loadScanRoots();
    if (!isTauri() || hasDevMock) return;

    let disposed = false;
    let unlistenDrop: (() => void) | null = null;
    let unlistenMenu: (() => void) | null = null;
    const appWindow = getCurrentWindow();
    void appWindow
      .onDragDropEvent(handleNativeFolderDrop)
      .then((stopListening) => {
        if (disposed) stopListening();
        else unlistenDrop = stopListening;
      })
      .catch(() => {});
    void appWindow
      .listen<unknown>("cepa://menu-command", ({ payload }) => {
        const command = desktopCommandForMenu(
          payload,
          currentDesktopCommandContext(),
        );
        if (command !== null) runDesktopCommand(command);
      })
      .then((stopListening) => {
        if (disposed) stopListening();
        else unlistenMenu = stopListening;
      })
      .catch(() => {});

    return () => {
      disposed = true;
      unlistenDrop?.();
      unlistenMenu?.();
    };
  });

  async function chooseDirectory() {
    if (isBusy) return;
    errorMessage = "";
    navigationError = "";
    revealError = "";
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Choose a folder to scan",
      });

      if (typeof selected === "string") {
        path = selected;
        await startScan();
      }
    } catch (error) {
      await showScanEntryError("Folder picker didn’t open.", String(error));
    }
  }

  async function startScan(event?: SubmitEvent) {
    event?.preventDefault();
    const requestedPath = path.trim();
    if (!requestedPath || isBusy) return;

    dropSequence += 1;
    invalidateCompletedScanRequests();
    preparingScanRoot = null;
    clearDropState();
    status = "scanning";
    path = requestedPath;
    scanStarted = false;
    scanActionError = "";
    errorHeading = "That scan didn’t start.";
    errorMessage = "";
    navigationError = "";
    navigationErrorTitle = "That folder could not be opened.";
    revealError = "";
    revealingNodeId = null;
    sizeMetric = "allocated";
    compressionCapability = null;
    if (import.meta.env.DEV) {
      delete document.documentElement.dataset.cepaScanRenderMs;
    }
    progress = null;
    result = null;
    view = null;
    selectedEntry = null;
    clearInspection();
    resetDirectorySearch(true);
    scanId = null;

    const onEvent = new Channel<ScanEvent>();
    onEvent.onmessage = (message) => {
      if (message.event === "started") {
        scanStarted = true;
        scanId = message.scanId;
      } else if (message.event === "progress") {
        scanStarted = true;
        scanId = message.scanId;
        progress = message.progress;
      }
    };

    try {
      const response = await invoke<ScanResponse>("scan_directory", {
        path: requestedPath,
        onEvent,
      });
      const renderStartedAt = performance.now();
      scanStarted = true;
      scanId = response.scanId;
      result = response.result;
      view = response.view;
      path = response.result.root;
      status = "complete";
      await tick();
      resultHeading?.focus();
      recordDevelopmentRender(renderStartedAt);
      void loadCompressionCapability(response.scanId);
    } catch (error) {
      scanActionError = "";
      const message = String(error);
      if (isCancellationError(message)) {
        status = "cancelled";
        scanId = null;
        errorMessage = "";
      } else {
        status = "error";
        scanId = null;
        errorHeading = scanStarted
          ? "The scan couldn’t finish."
          : "That scan didn’t start.";
        errorMessage = message;
      }
      await tick();
      stateNotice?.focus();
    }
  }

  function recordDevelopmentRender(startedAt: number) {
    if (!import.meta.env.DEV) return;
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        document.documentElement.dataset.cepaScanRenderMs = (
          performance.now() - startedAt
        ).toFixed(3);
      });
    });
  }

  async function cancelScan() {
    if (scanId === null || !isBusy) return;
    const cancellingScanId = scanId;
    scanActionError = "";
    status = "cancelling";
    try {
      await invoke("cancel_scan", { scanId: cancellingScanId });
    } catch (error) {
      if (scanId !== cancellingScanId || status !== "cancelling") return;
      status = "scanning";
      scanActionError = String(error);
      await tick();
      scanActionNotice?.focus();
    }
  }

  async function reset() {
    if (isBusy) return;
    invalidateCompletedScanRequests();
    const completedScanId = status === "complete" ? scanId : null;
    if (completedScanId !== null) {
      navigationError = "";
      revealError = "";
      isDiscardingScan = true;
      try {
        await invoke("discard_scan", { scanId: completedScanId });
      } catch (error) {
        if (scanId === completedScanId && status === "complete") {
          navigationErrorTitle = "This scan could not be closed.";
          navigationError = String(error);
          await tick();
          navigationNotice?.focus();
        }
        return;
      } finally {
        isDiscardingScan = false;
      }
    }

    dropSequence += 1;
    preparingScanRoot = null;
    clearDropState();
    status = "idle";
    scanStarted = false;
    scanActionError = "";
    errorHeading = "That scan didn’t start.";
    scanId = null;
    progress = null;
    result = null;
    view = null;
    selectedEntry = null;
    clearInspection();
    resetDirectorySearch(true);
    errorMessage = "";
    navigationError = "";
    navigationErrorTitle = "That folder could not be opened.";
    revealError = "";
    revealingNodeId = null;
    sizeMetric = "allocated";
    compressionCapability = null;
    await tick();
    landingHeading?.focus();
  }

  async function loadCompressionCapability(completedScanId: number) {
    try {
      const capability = await invoke<CompressionCapability>(
        "compression_capability",
        { scanId: completedScanId },
      );
      if (scanId === completedScanId && status === "complete") {
        compressionCapability = capability;
      }
    } catch (error) {
      if (scanId === completedScanId && status === "complete") {
        compressionCapability = {
          status: "unavailable",
          filesystem: "unknown",
          volumeSupportsTransparentCompression: false,
          writerAvailable: false,
          algorithms: [],
          detail: `The capability request failed: ${String(error)}`,
        };
      }
    }
  }

  function clearInspection() {
    clearEstimate();
    inspectionSequence += 1;
    inspectedEntry = null;
    compressionState = null;
    isInspectingCompression = false;
    inspectionReturnTarget = null;
  }

  async function closeInspection() {
    if (isDiscardingScan) return;
    const returnTarget = inspectionReturnTarget;
    clearInspection();
    await tick();
    if (returnTarget?.isConnected) returnTarget.focus();
  }

  async function inspectEntry(entry: ChartItem | ScanItem) {
    if (scanId === null || entry.id === null || isResultBusy) return;
    clearEstimate();
    const completedScanId = scanId;
    const request = ++inspectionSequence;
    inspectedEntry = entry;
    compressionState = null;
    isInspectingCompression = true;
    await tick();
    if (inspectionSequence === request && inspectedEntry?.id === entry.id) {
      keepInspectionTargetVisible(inspectionReturnTarget);
    }
    try {
      const state = await invoke<CompressionState>("compression_state", {
        scanId: completedScanId,
        nodeId: entry.id,
      });
      if (
        inspectionSequence === request &&
        scanId === completedScanId &&
        inspectedEntry?.id === entry.id
      ) {
        compressionState = state;
      }
    } catch (error) {
      if (
        inspectionSequence === request &&
        scanId === completedScanId &&
        inspectedEntry?.id === entry.id
      ) {
        compressionState = {
          state: "unavailable",
          scope: "none",
          format: null,
          detail: `Compression couldn’t be checked: ${String(error)}`,
        };
      }
    } finally {
      if (inspectionSequence === request) {
        isInspectingCompression = false;
        await tick();
        if (inspectedEntry?.id === entry.id) {
          keepInspectionTargetVisible(inspectionReturnTarget);
        }
      }
    }
  }

  function keepInspectionTargetVisible(
    target: (HTMLElement | SVGGElement) | null,
  ) {
    const list = target?.closest<HTMLElement>(".item-list");
    if (!target || !list) return;

    const targetBounds = target.getBoundingClientRect();
    const listBounds = list.getBoundingClientRect();
    if (targetBounds.top < listBounds.top) {
      list.scrollTop -= listBounds.top - targetBounds.top;
    } else if (targetBounds.bottom > listBounds.bottom) {
      list.scrollTop += targetBounds.bottom - listBounds.bottom;
    }
  }

  function clearEstimate() {
    if (activeEstimateRequestId !== null && isEstimatingSavings) {
      void invoke("cancel_compression_estimate", {
        requestId: activeEstimateRequestId,
      }).catch(() => {});
    }
    savingsEstimate = null;
    isEstimatingSavings = false;
    isCancellingEstimate = false;
    estimateActionError = "";
    activeEstimateRequestId = null;
  }

  async function estimateSavings() {
    if (
      scanId === null ||
      inspectedEntry === null ||
      inspectedEntry.id === null ||
      isEstimatingSavings ||
      isResultBusy
    ) return;
    const completedScanId = scanId;
    const nodeId = inspectedEntry.id;
    const requestId = ++estimateRequestSequence;
    activeEstimateRequestId = requestId;
    savingsEstimate = null;
    isEstimatingSavings = true;
    isCancellingEstimate = false;
    estimateActionError = "";
    try {
      await tick();
      estimateCancelButton?.focus();
      const estimate = await invoke<SavingsEstimate>("estimate_compression_savings", {
        scanId: completedScanId,
        nodeId,
        requestId,
      });
      if (
        activeEstimateRequestId === requestId &&
        scanId === completedScanId &&
        inspectedEntry?.id === nodeId
      ) {
        savingsEstimate = estimate;
      }
    } catch (error) {
      if (
        activeEstimateRequestId === requestId &&
        scanId === completedScanId &&
        inspectedEntry?.id === nodeId
      ) {
        savingsEstimate = {
          status: "unavailable",
          algorithm: null,
          fidelity: "none",
          confidence: "none",
          sampledBytes: 0,
          logicalBytes: inspectedEntry.logicalBytes,
          allocatedBytes: inspectedEntry.allocatedBytes,
          estimatedSavingsLower: null,
          estimatedSavingsUpper: null,
          estimatorVersion: 1,
          detail: `Savings couldn’t be estimated: ${String(error)}`,
        };
      }
    } finally {
      if (activeEstimateRequestId === requestId) {
        isEstimatingSavings = false;
        isCancellingEstimate = false;
        estimateActionError = "";
        activeEstimateRequestId = null;
        await tick();
        estimateActionButton?.focus();
      }
    }
  }

  async function cancelEstimate() {
    if (
      activeEstimateRequestId === null ||
      !isEstimatingSavings ||
      isDiscardingScan
    ) return;
    const requestId = activeEstimateRequestId;
    estimateActionError = "";
    isCancellingEstimate = true;
    try {
      await invoke("cancel_compression_estimate", {
        requestId,
      });
    } catch (error) {
      if (activeEstimateRequestId !== requestId || !isEstimatingSavings) return;
      isCancellingEstimate = false;
      estimateActionError = String(error);
      await tick();
      estimateActionNotice?.focus();
    }
  }

  function itemPercent(bytes: number): number {
    if (bytes <= 0 || viewBytes <= 0) return 0;
    return Math.max(0.8, Math.min(100, (bytes / viewBytes) * 100));
  }

  function invalidateDirectorySearch() {
    if (searchTimer !== null) {
      clearTimeout(searchTimer);
      searchTimer = null;
    }
    if (activeSearchRequestId !== null) {
      void invoke("cancel_directory_search", {
        requestId: activeSearchRequestId,
      }).catch(() => {});
      activeSearchRequestId = null;
    }
    searchSequence += 1;
    searchResult = null;
    searchError = "";
    isSearching = false;
  }

  function resetDirectorySearch(close: boolean) {
    invalidateDirectorySearch();
    searchQuery = "";
    if (close) searchOpen = false;
  }

  async function openDirectorySearch() {
    if (isResultBusy) return;
    searchOpen = true;
    await tick();
    searchInput?.focus();
  }

  async function closeDirectorySearch() {
    resetDirectorySearch(true);
    await tick();
    searchToggleButton?.focus();
  }

  function clearDirectorySearch() {
    searchQuery = "";
    invalidateDirectorySearch();
    searchInput?.focus();
  }

  function handleSearchKeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      void closeDirectorySearch();
    }
  }

  function currentDesktopMenuAvailability(): DesktopMenuAvailability {
    return {
      canChooseDirectory: !isBusy,
      canRescan: status === "complete" && !isBusy,
      canSearch:
        status === "complete" && view !== null && !isNavigating && !isBusy,
      canNavigateUp:
        status === "complete" && parentId !== null && !isNavigating && !isBusy,
    };
  }

  function currentDesktopCommandContext(): DesktopCommandContext {
    return {
      ...currentDesktopMenuAvailability(),
      primaryModifier,
      searchOpen,
      detailsOpen: inspectedEntry !== null,
    };
  }

  function runDesktopCommand(command: DesktopCommand) {
    switch (command) {
      case "chooseDirectory":
        void chooseDirectory();
        break;
      case "rescan":
        void startScan();
        break;
      case "search":
        void openDirectorySearch();
        break;
      case "navigateUp":
        void openDirectory(parentId);
        break;
      case "closeSearch":
        void closeDirectorySearch();
        break;
      case "closeDetails":
        void closeInspection();
        break;
      case "suppress":
        break;
    }
  }

  function handleDesktopKeydown(event: KeyboardEvent) {
    const command = desktopCommandForKeydown(
      event,
      currentDesktopCommandContext(),
    );
    if (command === null) return;

    event.preventDefault();
    runDesktopCommand(command);
  }

  function scheduleDirectorySearch() {
    invalidateDirectorySearch();
    const query = searchQuery.trim();
    if (!query || scanId === null || !view || isResultBusy) return;

    isSearching = true;
    const sequence = searchSequence;
    const completedScanId = scanId;
    const nodeId = view.nodeId;
    const metric = sizeMetric;
    searchTimer = setTimeout(() => {
      searchTimer = null;
      void runDirectorySearch(sequence, completedScanId, nodeId, metric, query);
    }, 180);
  }

  async function runDirectorySearch(
    sequence: number,
    completedScanId: number,
    nodeId: number,
    metric: SizeMetric,
    query: string,
  ) {
    if (searchSequence !== sequence) return;
    activeSearchRequestId = sequence;
    try {
      const result = await invoke<DirectorySearchResult>("search_scan_directory", {
        scanId: completedScanId,
        nodeId,
        metric,
        query,
        requestId: sequence,
      });
      if (
        searchSequence === sequence &&
        scanId === completedScanId &&
        view?.nodeId === nodeId &&
        sizeMetric === metric &&
        searchQuery.trim() === query
      ) {
        searchResult = result;
      }
    } catch (error) {
      if (
        searchSequence === sequence &&
        scanId === completedScanId &&
        view?.nodeId === nodeId &&
        sizeMetric === metric &&
        searchQuery.trim() === query
      ) {
        searchError = String(error);
      }
    } finally {
      if (activeSearchRequestId === sequence) activeSearchRequestId = null;
      if (searchSequence === sequence) isSearching = false;
    }
  }

  async function loadDirectory(
    nodeId: number,
    metric: SizeMetric,
    focusHeading: boolean,
  ) {
    if (scanId === null || isResultBusy) return;
    const completedScanId = scanId;
    const request = {
      scanId: completedScanId,
      sequence: ++navigationSequence,
    };
    const preservedSearch = focusHeading ? "" : searchQuery;
    if (focusHeading) resetDirectorySearch(true);
    else invalidateDirectorySearch();
    isNavigating = true;
    navigationError = "";
    revealError = "";
    try {
      const nextView = await invoke<DirectoryView>("open_scan_directory", {
        scanId: completedScanId,
        nodeId,
        metric,
      });
      if (
        !isCurrentCompletedScanRequest(request, {
          scanId,
          sequence: navigationSequence,
          complete: status === "complete",
        })
      ) return;
      view = nextView;
      sizeMetric = metric;
      selectedEntry = null;
      clearInspection();
      if (preservedSearch.trim()) {
        searchQuery = preservedSearch;
        scheduleDirectorySearch();
      }
      if (focusHeading) {
        await tick();
        viewHeading?.focus();
      }
    } catch (error) {
      if (
        !isCurrentCompletedScanRequest(request, {
          scanId,
          sequence: navigationSequence,
          complete: status === "complete",
        })
      ) return;
      navigationErrorTitle = focusHeading
        ? "That folder could not be opened."
        : "The size metric could not be changed.";
      navigationError = String(error);
      await tick();
      navigationNotice?.focus();
    } finally {
      if (navigationSequence === request.sequence) isNavigating = false;
    }
  }

  async function openDirectory(nodeId: number | null) {
    if (nodeId === null) return;
    await loadDirectory(nodeId, sizeMetric, true);
  }

  async function setSizeMetric(metric: SizeMetric) {
    if (!view || metric === sizeMetric) return;
    await loadDirectory(view.nodeId, metric, false);
  }

  async function revealItem(nodeId: number) {
    if (scanId === null || revealingNodeId !== null || isResultBusy) return;
    const completedScanId = scanId;
    const request = {
      scanId: completedScanId,
      sequence: ++revealSequence,
    };
    revealingNodeId = nodeId;
    revealError = "";
    try {
      await invoke("reveal_scan_item", { scanId: completedScanId, nodeId });
    } catch (error) {
      if (
        !isCurrentCompletedScanRequest(request, {
          scanId,
          sequence: revealSequence,
          complete: status === "complete",
        })
      ) return;
      revealError = String(error);
      await tick();
      revealNotice?.focus();
    } finally {
      if (revealSequence === request.sequence) revealingNodeId = null;
    }
  }

  function invalidateCompletedScanRequests() {
    // Backend scan IDs authorize each command; these frontend generations also
    // prevent an older completion from mutating a newer UI lifecycle.
    navigationSequence += 1;
    isNavigating = false;
    revealSequence += 1;
    revealingNodeId = null;
  }

  function activateEntry(
    entry: ChartItem | ScanItem,
    trigger?: HTMLElement | SVGGElement,
  ) {
    if (entry.kind === "directory") {
      void openDirectory(entry.id);
    } else {
      selectedEntry = entry;
      inspectionReturnTarget = trigger ?? null;
      void inspectEntry(entry);
    }
  }

  function previewEntry(entry: ChartItem | ScanItem) {
    if (selectedEntry?.id !== entry.id) selectedEntry = entry;
  }

  function handleSegmentKeydown(event: KeyboardEvent, entry: ChartItem) {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      activateEntry(entry, event.currentTarget as SVGGElement);
      return;
    }

    const targetId = sunburstNavigationTarget(
      sunburstSegments,
      chartFocusId,
      event.key,
    );
    if (targetId === null) return;
    event.preventDefault();
    chartFocusId = targetId;
    requestAnimationFrame(() => {
      sunburstElement
        ?.querySelector<SVGGElement>(`[data-chart-node-id="${targetId}"]`)
        ?.focus();
    });
  }
</script>

<svelte:window onkeydown={handleDesktopKeydown} />

<svelte:head>
  <title>Cepa — Disk space, clearly</title>
  <meta
    name="description"
    content="Cepa is a fast, focused disk space analyzer for your desktop."
  />
</svelte:head>

<div class="app-shell">
  <header
    class="app-header"
    inert={dropOverlayVisible}
    aria-hidden={dropOverlayVisible ? "true" : undefined}
  >
    <button
      class="wordmark"
      type="button"
      disabled={isBusy}
      onclick={reset}
      aria-label="Cepa home"
    >
      <CepaMark class="wordmark-mark" />
      <span>Cepa</span>
    </button>

    {#if status === "complete"}
      <div class="header-actions">
        <Button
          variant="ghost"
          size="sm"
          class="scan-again-action"
          disabled={isBusy}
          aria-label="Scan again"
          title={`Scan again (${primaryShortcutLabel}R)`}
          onclick={() => startScan()}
        >
          <RefreshCw data-icon="inline-start" />
          <span class="scan-again-label">Scan again</span>
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={isBusy}
          title={`Choose folder (${primaryShortcutLabel}O)`}
          onclick={chooseDirectory}
        >
          <FolderOpen data-icon="inline-start" />
          Choose folder
        </Button>
      </div>
    {/if}
  </header>

  {#if dropOverlayVisible}
    <div
      class="folder-drop-overlay"
      data-state={isPreparingDroppedFolder ? "preparing" : "ready"}
      role="status"
      aria-live="polite"
    >
      <div class="folder-drop-copy">
        <span class="folder-drop-symbol" aria-hidden="true"><FolderDown /></span>
        <strong>
          {isPreparingDroppedFolder
            ? `Opening ${droppedFolderLabel}…`
            : droppedPaths.length === 1
              ? `Scan ${droppedFolderLabel}`
              : "One folder at a time"}
        </strong>
        <span>
          {isPreparingDroppedFolder
            ? "Checking this folder"
            : droppedPaths.length === 1
              ? "Release to explore its contents"
              : "Drop a single folder to scan"}
        </span>
      </div>
    </div>
  {/if}

  {#if status === "idle" || status === "error" || status === "cancelled"}
    <main
      class="landing"
      class:landing-with-storage={showsScanRoots}
      inert={dropOverlayVisible}
      aria-hidden={dropOverlayVisible ? "true" : undefined}
    >
      <section class="landing-copy" aria-labelledby="landing-title">
        <div class="app-symbol" aria-hidden="true"><CepaMark /></div>
        <h1 id="landing-title" tabindex="-1" bind:this={landingHeading}>
          Find what’s taking up space.
        </h1>
        <p class="lede">
          Choose a disk or folder to see its largest files and subfolders. Everything
          stays on this device.
        </p>

        <ScanRootPicker
          roots={scanRoots}
          status={scanRootsStatus}
          busy={isBusy}
          preparingPath={preparingScanRoot}
          onSelect={(rootPath) => void startValidatedRoot(rootPath, false)}
          onRetry={() => void loadScanRoots()}
        />

        <div class="scan-entry">
          <Button
            class="choose-button"
            variant={scanRoots.length > 0 ? "outline" : "default"}
            size="lg"
            disabled={isBusy}
            title={`Choose folder (${primaryShortcutLabel}O)`}
            onclick={chooseDirectory}
          >
            <FolderOpen data-icon="inline-start" />
            Choose folder…
          </Button>

          <details class="manual-path">
            <summary>Enter a path instead</summary>
            <form class="path-form" onsubmit={startScan}>
              <label class="sr-only" for="scan-path">Folder path</label>
              <Input
                id="scan-path"
                class="path-input"
                placeholder={scanPathPlaceholder}
                autocomplete="off"
                spellcheck={false}
                disabled={isBusy}
                bind:value={path}
              />
              <Button
                variant="outline"
                type="submit"
                disabled={!path.trim() || isBusy}
              >
                Scan
              </Button>
            </form>
          </details>

          {#if status === "error" && errorMessage}
            <div
              class="error-callout"
              role="alert"
              tabindex="-1"
              bind:this={stateNotice}
            >
              <AlertCircle />
              <div>
                <strong>{errorHeading}</strong>
                <span>{errorMessage}</span>
              </div>
            </div>
          {:else if status === "cancelled"}
            <div
              class="cancelled-callout"
              role="status"
              tabindex="-1"
              bind:this={stateNotice}
            >
              <CircleStop />
              <div>
                <strong>Scan stopped.</strong>
                <span>
                  {formatCount(displayProgress.entriesScanned)} items and
                  {formatBytes(displayProgress.allocatedBytes)} were found before it stopped.
                </span>
              </div>
            </div>
          {/if}
        </div>
      </section>
    </main>
  {:else if status === "scanning" || status === "cancelling"}
    <main
      class="scan-view"
      aria-busy="true"
      inert={dropOverlayVisible}
      aria-hidden={dropOverlayVisible ? "true" : undefined}
    >
      <p class="sr-only" aria-live="polite">{progressPresentation.announcement}</p>
      <section class="scan-progress" aria-labelledby="scan-progress-title">
        <div class="scan-titlebar">
          <div>
            <span class="scan-kicker">
              <span class="status-dot" aria-hidden="true"></span>
              {progressPresentation.statusLabel}
            </span>
            <h1 id="scan-progress-title">{scanTargetName}</h1>
          </div>
          <Button
            variant="outline"
            size="sm"
            onclick={cancelScan}
            disabled={scanId === null || status === "cancelling"}
          >
            {status === "cancelling" ? "Stopping…" : "Stop"}
          </Button>
        </div>

        {#if scanActionError}
          <div
            class="error-callout"
            role="alert"
            tabindex="-1"
            bind:this={scanActionNotice}
          >
            <AlertCircle />
            <div>
              <strong>Couldn’t stop the scan.</strong>
              <span>{scanActionError} The scan is still running; try again.</span>
            </div>
          </div>
        {/if}

        <div class="scan-total">
          <span>{progressPresentation.totalLabel}</span>
          <strong>{formatBytes(displayProgress.allocatedBytes)}</strong>
        </div>
        <p
          class="scan-path"
          title={progressPresentation.currentLabel ? undefined : displayProgress.currentPath}
        >
          {progressPresentation.currentLabel ?? (displayProgress.currentPath || path)}
        </p>

        <div class="scan-line" aria-hidden="true"><span></span></div>

        <dl class="scan-facts" aria-label="Scan progress">
          <div><dt>Items scanned</dt><dd>{formatCount(displayProgress.entriesScanned)}</dd></div>
          <div><dt>Elapsed</dt><dd>{formatDuration(displayProgress.elapsedMs)}</dd></div>
          {#if displayProgress.skippedEntries > 0}
            <div class="scan-fact-exception">
              <dt>Unavailable items</dt>
              <dd>{formatCount(displayProgress.skippedEntries)}</dd>
            </div>
          {/if}
        </dl>

        {#if displayProgress.largestItems.length > 0}
          <section class="partial-results" aria-label="Largest files observed so far">
            <h2>Largest so far</h2>
            <ol>
              {#each displayProgress.largestItems.slice(0, 4) as item (item.id)}
                <li>
                  <File aria-hidden="true" />
                  <strong title={item.name}>{item.name}</strong>
                  <span>{formatBytes(item.allocatedBytes)}</span>
                </li>
              {/each}
            </ol>
          </section>
        {:else}
          <div class="scan-empty-progress">
            <ScanSearch aria-hidden="true" />
            <span>Looking for files…</span>
          </div>
        {/if}
      </section>
    </main>
  {:else if result && view}
    <main
      class="results-view"
      inert={dropOverlayVisible}
      aria-hidden={dropOverlayVisible ? "true" : undefined}
    >
      <section class="results-heading">
        <div class="result-title">
          <h1 tabindex="-1" bind:this={resultHeading}>{result.displayName}</h1>
          <p class="result-path" title={result.root}>{result.root}</p>
        </div>
        <div class="result-total">
          <span>Space on disk{result.allocatedSizeIsEstimate ? " (estimated)" : ""}</span>
          <strong>{formatBytes(result.allocatedBytes)}</strong>
        </div>
      </section>

      <p class="result-summary" aria-label="Scan summary">
        <span>{formatBytes(result.logicalBytes)} logical</span>
        <span>{formatCount(result.fileCount)} files</span>
        <span>{formatCount(result.directoryCount)} folders</span>
        <span>{formatDuration(result.elapsedMs)}</span>
      </p>

      {#if result.skippedEntries > 0}
        <div class="coverage-notice" role="status">
          <AlertCircle aria-hidden="true" />
          <p>
            <strong>Some items weren’t included.</strong>
            <span>{formatUnavailableItems(result.skippedEntries)}, so totals may be lower than the space actually in use.</span>
          </p>
        </div>
      {/if}

      <div class="explorer-toolbar">
        <nav class="breadcrumbs" aria-label="Current scan path">
          {#each view.breadcrumbs as breadcrumb, index (breadcrumb.id)}
            {#if index > 0}<ChevronRight aria-hidden="true" />{/if}
            <button
              type="button"
              disabled={isResultBusy}
              aria-current={index === view.breadcrumbs.length - 1 ? "page" : undefined}
              onclick={() => openDirectory(breadcrumb.id)}
            >{breadcrumb.name}</button>
          {/each}
        </nav>
        <div class="metric-switch" role="group" aria-label="Size metric">
          <button
            type="button"
            aria-pressed={sizeMetric === "allocated"}
            aria-disabled={isResultBusy}
            onclick={() => setSizeMetric("allocated")}
          >On disk</button>
          <button
            type="button"
            aria-pressed={sizeMetric === "logical"}
            aria-disabled={isResultBusy}
            onclick={() => setSizeMetric("logical")}
          >Logical</button>
        </div>
      </div>

      {#if navigationError}
        <div
          class="error-callout navigation-error"
          role="alert"
          tabindex="-1"
          bind:this={navigationNotice}
        >
          <AlertCircle />
          <div>
            <strong>{navigationErrorTitle}</strong>
            <span>{navigationError}</span>
          </div>
        </div>
      {/if}

      {#if revealError}
        <div
          class="error-callout navigation-error"
          role="alert"
          tabindex="-1"
          bind:this={revealNotice}
        >
          <AlertCircle />
          <div>
            <strong>That item could not be shown.</strong>
            <span>{revealError}</span>
          </div>
        </div>
      {/if}

      <section
        class="explorer"
        aria-label="Storage map and folder contents"
        aria-busy={isResultBusy}
      >
        <div class="chart-pane">
          <h2 class="sr-only" tabindex="-1" bind:this={viewHeading}>
            Storage map for {view.displayName}
          </h2>
          {#if view.path !== view.root}
            <Button
              class="chart-back"
              variant="ghost"
              size="sm"
              disabled={isResultBusy}
              title={`Up (${backShortcutLabel})`}
              onclick={() => openDirectory(parentId)}
            >
              <ArrowLeft data-icon="inline-start" /> Up
            </Button>
          {/if}

          <div class="sunburst-wrap">
            {#if sunburstSegments.length > 0}
              <svg
                class="sunburst"
                viewBox="0 0 340 340"
                role="group"
                aria-label={`Storage map for ${view.displayName} by ${formatMetric(sizeMetric).toLowerCase()}`}
                aria-describedby="sunburst-navigation-help"
                bind:this={sunburstElement}
              >
                {#each sunburstSegments as segment (`${segment.item.id ?? segment.item.name}-${segment.depth}`)}
                  {#if segment.item.id !== null}
                    <g
                      role="button"
                      tabindex={chartFocusId === segment.item.id ? 0 : -1}
                      data-chart-node-id={segment.item.id}
                      aria-label={`${segment.item.name}, ${formatBytes(metricBytes(segment.item, sizeMetric))}`}
                      onpointermove={() => previewEntry(segment.item)}
                      onfocus={() => {
                        chartFocusId = segment.item.id;
                        selectedEntry = segment.item;
                      }}
                      onmouseleave={() => (selectedEntry = null)}
                      onblur={() => (selectedEntry = null)}
                      onclick={(event) => activateEntry(segment.item, event.currentTarget)}
                      onkeydown={(event) => handleSegmentKeydown(event, segment.item)}
                    >
                      <path
                        d={segment.pathData}
                        data-depth={segment.depth}
                        data-color={segment.colorIndex}
                        data-selected={activeEntry?.id === segment.item.id}
                      />
                    </g>
                  {:else}
                    <path
                      d={segment.pathData}
                      data-depth={segment.depth}
                      data-color={segment.colorIndex}
                      data-selected={activeEntry?.id === segment.item.id}
                    />
                  {/if}
                {/each}
              </svg>
            {:else}
              <div class="chart-empty"><Folder /></div>
            {/if}

            <div class="chart-center" aria-live="polite">
              <strong>{formatBytes(activeEntry ? metricBytes(activeEntry, sizeMetric) : viewBytes)}</strong>
              <em>{activeEntry?.name ?? view.displayName}</em>
            </div>
          </div>

          {#if sunburstSegments.length > 0}
            <p id="sunburst-navigation-help" class="sr-only">
              Use the arrow keys to move between segments. Press Enter or Space to open the selected item.
            </p>
            <p class="chart-help">Select a segment to explore it</p>
          {/if}
        </div>

        <div class="directory-pane">
          <div class="section-heading">
            <h2>{view.displayName}</h2>
            <div class="section-actions">
              <span
                id="directory-search-status"
                class="directory-count"
                aria-live="polite"
                aria-atomic="true"
              >{directoryCountLabel}</span>
              {#if searchOpen}
                <div class="directory-search">
                  <Search aria-hidden="true" />
                  <Input
                    type="search"
                    placeholder="Find in this folder"
                    aria-label={`Find in ${view.displayName}`}
                    aria-describedby="directory-search-status"
                    maxlength={128}
                    autocomplete="off"
                    spellcheck={false}
                    bind:ref={searchInput}
                    bind:value={searchQuery}
                    oninput={scheduleDirectorySearch}
                    onkeydown={handleSearchKeydown}
                  />
                  {#if searchActive}
                    <Button
                      variant="ghost"
                      size="icon-xs"
                      aria-label="Clear folder search"
                      title="Clear"
                      onclick={clearDirectorySearch}
                    ><X /></Button>
                  {/if}
                </div>
              {:else}
                <Button
                  variant="ghost"
                  size="icon-xs"
                  bind:ref={searchToggleButton}
                  disabled={isResultBusy}
                  aria-label="Search this folder"
                  title={`Search this folder (${primaryShortcutLabel}F)`}
                  onclick={openDirectorySearch}
                ><Search /></Button>
              {/if}
            </div>
          </div>

          {#if inspectedEntry}
            <section
              class="selection-inspector"
              aria-label={`Compression details for ${inspectedEntry.name}`}
              aria-busy={isInspectingCompression}
              aria-live="polite"
            >
              <header class="inspector-heading">
                <div>
                  <span>{inspectedEntry.kind === "file" ? "File details" : "Item details"}</span>
                  <strong title={inspectedEntry.name}>{inspectedEntry.name}</strong>
                </div>
                <div class="inspector-actions">
                  {#if isEstimatingSavings}
                    <Button
                      variant="outline"
                      size="xs"
                      disabled={isCancellingEstimate}
                      bind:ref={estimateCancelButton}
                      aria-label="Cancel savings estimate"
                      onclick={cancelEstimate}
                    >Cancel</Button>
                  {/if}
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    aria-label="Close item details"
                    title="Close (Esc)"
                    onclick={closeInspection}
                  ><X /></Button>
                </div>
              </header>
              <div class="inspector-body">
                <div class="inspection-state">
                  <span>Compression</span>
                  <strong data-state={compressionState?.state ?? "loading"}>
                    {isInspectingCompression
                      ? "Checking…"
                      : compressionState
                        ? formatCompressionState(compressionState)
                        : "Couldn’t be checked"}
                  </strong>
                </div>
                {#if isEstimatingSavings}
                  <div class="estimate-readout estimate-readout-active" aria-live="polite">
                    <div>
                      <span>Potential savings</span>
                      <strong>{isCancellingEstimate ? "Stopping…" : "Estimating…"}</strong>
                    </div>
                    {#if estimateActionError}
                      <div
                        class="estimate-action-error"
                        role="alert"
                        tabindex="-1"
                        title={estimateActionError}
                        bind:this={estimateActionNotice}
                      >
                        <AlertCircle aria-hidden="true" />
                        <p>
                          <strong>Couldn’t stop estimating.</strong>
                          <span>The estimate is still running. Try again.</span>
                        </p>
                      </div>
                    {/if}
                  </div>
                {:else if savingsEstimate}
                  <div class="estimate-readout" data-status={savingsEstimate.status}>
                    <div>
                      <span>Potential savings</span>
                      <strong>{formatSavingsEstimate(savingsEstimate)}</strong>
                    </div>
                    {#if canEstimateSavings}
                      <Button
                        variant="outline"
                        size="sm"
                        bind:ref={estimateActionButton}
                        onclick={estimateSavings}
                      >Estimate again</Button>
                    {/if}
                    <details class="metadata-disclosure">
                      <summary>Estimate details</summary>
                      {#if savingsEstimate.algorithm}
                        <p>
                          {savingsEstimate.algorithm} · {savingsEstimate.fidelity === "exact" ? "target codec" : "proxy codec"} · {savingsEstimate.confidence} confidence · {formatBytes(savingsEstimate.sampledBytes)} sampled
                        </p>
                      {/if}
                      <p>{savingsEstimate.detail}</p>
                    </details>
                  </div>
                {:else if canEstimateSavings}
                  <div class="estimate-prompt">
                    <span>See how much space compression might save.</span>
                    <Button
                      variant="outline"
                      size="sm"
                      bind:ref={estimateActionButton}
                      onclick={estimateSavings}
                    >Estimate</Button>
                  </div>
                {/if}
                {#if compressionState}
                  <details class="metadata-disclosure">
                    <summary>Compression details</summary>
                    <p>{compressionState.detail}</p>
                  </details>
                {/if}
              </div>
            </section>
          {/if}

          {#if searchActive && searchError}
            <div class="search-message" role="alert">
              <AlertCircle />
              <strong>Search unavailable</strong>
              <span>{searchError}</span>
              <Button variant="outline" size="sm" onclick={clearDirectorySearch}>Clear search</Button>
            </div>
          {:else if searchActive && isSearching && !searchResult}
            <div class="search-message">
              <Search />
              <strong>Searching this folder…</strong>
            </div>
          {:else if searchActive && !isSearching && searchResult?.totalMatches === 0}
            <div class="search-message">
              <Search />
              <strong>No matches in this folder</strong>
              <span>Try a shorter or different name.</span>
              <Button variant="outline" size="sm" onclick={clearDirectorySearch}>Clear search</Button>
            </div>
          {:else if visibleItems.length > 0}
            <p id="directory-list-navigation-help" class="sr-only">
              Use the Up and Down Arrow keys to move between items. Home and End jump to the first and last item. Press Enter to open a folder or show file details. When available, Tab once for the Reveal action.
            </p>
            <div
              class="item-list"
              class:is-navigating={isResultBusy || isSearching}
              role="list"
              aria-label={`${view.displayName} contents`}
              aria-describedby="directory-list-navigation-help"
              aria-busy={isSearching}
              bind:this={itemListElement}
            >
              {#each visibleItems as item (item.id)}
                <div
                  class="storage-row"
                  role="listitem"
                  data-selected={activeEntry?.id === item.id}
                  data-inspected={inspectedEntry?.id === item.id}
                >
                  <button
                    type="button"
                    class="storage-item"
                    tabindex={listFocusId === item.id ? 0 : -1}
                    data-list-open-id={item.id}
                    disabled={isResultBusy}
                    onclick={(event) => activateEntry(item, event.currentTarget)}
                    onpointermove={() => previewEntry(item)}
                    onfocus={() => {
                      listFocusId = item.id;
                      selectedEntry = item;
                    }}
                    onmouseleave={() => (selectedEntry = null)}
                    onblur={() => (selectedEntry = null)}
                    onkeydown={(event) => handleListNavigation(event, item.id, "open")}
                    aria-label={`${item.name}, ${formatBytes(metricBytes(item, sizeMetric))}${item.kind === "directory" ? ", open folder" : ", show details"}`}
                  >
                    <span class="item-icon" data-kind={item.kind}>
                      {#if item.kind === "directory"}
                        <Folder />
                      {:else if item.kind === "symlink"}
                        <Link2 />
                      {:else}
                        <File />
                      {/if}
                    </span>
                    <span class="item-copy">
                      <strong title={item.name}>{item.name}</strong>
                      <span>
                        {describeEntry(item)}
                      </span>
                      <progress
                        class="item-bar"
                        max="100"
                        value={itemPercent(metricBytes(item, sizeMetric))}
                        aria-hidden="true"
                      ></progress>
                    </span>
                    <span class="item-size">
                      <strong>{formatBytes(metricBytes(item, sizeMetric))}</strong>
                      <span>{formatPercent(metricBytes(item, sizeMetric), viewBytes)}</span>
                    </span>
                    {#if item.kind === "directory"}<ChevronRight class="item-chevron" />{/if}
                  </button>
                  {#if item.kind !== "symlink"}
                    <button
                      type="button"
                      class="reveal-item"
                      tabindex={listFocusId === item.id ? 0 : -1}
                      data-list-reveal-id={item.id}
                      disabled={isResultBusy || revealingNodeId !== null}
                      aria-label={`Reveal ${item.name} in the system file manager`}
                      title="Reveal in file manager"
                      onclick={() => revealItem(item.id)}
                      onpointermove={() => previewEntry(item)}
                      onfocus={() => {
                        listFocusId = item.id;
                        selectedEntry = item;
                      }}
                      onblur={() => (selectedEntry = null)}
                      onkeydown={(event) => handleListNavigation(event, item.id, "reveal")}
                    >
                      <FolderSearch />
                    </button>
                  {/if}
                </div>
              {/each}
            </div>
          {:else}
            <div class="empty-result">
              <Folder />
              <strong>This folder is empty</strong>
            </div>
          {/if}
        </div>
      </section>

      <details class="scan-details">
        <summary>
          <strong>Scan details</strong>
          <ChevronRight aria-hidden="true" />
        </summary>
        <dl>
          <div><dt>Scanner</dt><dd>{formatBackend(result.backend)}</dd></div>
          <div><dt>Space on disk</dt><dd>{result.allocatedSizeIsEstimate ? "Estimated" : "Exact"}</dd></div>
          <div><dt>Hard links</dt><dd>{result.hardLinkDeduplicationSupported ? "Counted once" : "Not deduplicated"}</dd></div>
          <div><dt>Other filesystems</dt><dd>{result.sameFilesystemEnforced ? "Not traversed" : "Boundary unavailable"}</dd></div>
          <div><dt>Items not included</dt><dd>{formatCount(result.skippedEntries)}</dd></div>
          <div><dt>Mounted filesystems skipped</dt><dd>{formatCount(result.skippedFilesystems)}</dd></div>
          {#if result.duplicateHardLinks > 0}
            <div><dt>Duplicate hard links</dt><dd>{formatCount(result.duplicateHardLinks)}</dd></div>
          {/if}
          {#if compressionCapability}
            <div class="compression-detail">
              <dt>Filesystem compression</dt>
              <dd title={compressionCapability.detail} aria-live="polite">
                {compressionCapability.status === "inspectOnly"
                  ? "Available for analysis"
                  : compressionCapability.status === "unsupported"
                    ? "Not supported"
                    : "Couldn’t be checked"}
              </dd>
            </div>
          {/if}
        </dl>
      </details>
    </main>
  {/if}
</div>
