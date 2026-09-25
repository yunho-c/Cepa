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
    Info,
    Link2,
    Search,
    TriangleAlert,
    X,
  } from "@lucide/svelte";
  import { Button, buttonVariants } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import * as Popover from "$lib/components/ui/popover";
  import * as Tooltip from "$lib/components/ui/tooltip";
  import type { AppStatus } from "$lib/app-shell";
  import CepaMark from "$lib/components/cepa-mark.svelte";
  import WindowTitlebar from "$lib/components/window-titlebar.svelte";
  import ScanRootPicker from "$lib/components/scan-root-picker.svelte";
  import { isCurrentCompletedScanRequest } from "$lib/completed-scan-request";
  import { droppedItemName, folderDropAction } from "$lib/folder-drop";
  import {
    listNavigationActionTarget,
    listNavigationTarget,
    type ListNavigationAction,
  } from "$lib/list-navigation";
  import { inspectorAnnouncement } from "$lib/inspector-announcement";
  import {
    navigationRecoveryMessage,
    type NavigationRecovery,
  } from "$lib/result-action-recovery";
  import {
    scanEntryRecoveryMessage,
    scanFailurePresentation,
  } from "$lib/recovery-copy";
  import {
    scanRootsPreviewStatus,
    shouldShowScanRoots,
    type ScanRoot,
    type ScanRootsStatus,
  } from "$lib/scan-roots";
  import {
    nextScanProgressAnnouncement,
    type ScanProgressAnnouncementCheckpoint,
  } from "$lib/scan-progress-announcement";
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

  const chartInteractionKeys = new Set([
    "Enter",
    " ",
    "ArrowLeft",
    "ArrowRight",
    "ArrowUp",
    "ArrowDown",
    "Home",
    "End",
  ]);

  let path = $state("");
  let status = $state<AppStatus>("idle");
  let scanId = $state<number | null>(null);
  let progress = $state<ScanProgress | null>(null);
  let result = $state<ScanResult | null>(null);
  let view = $state<DirectoryView | null>(null);
  let pointerEntry = $state<ChartItem | ScanItem | null>(null);
  let focusedEntry = $state<ChartItem | ScanItem | null>(null);
  let inspectedEntry = $state<ChartItem | ScanItem | null>(null);
  let isNavigating = $state(false);
  let navigationSequence = 0;
  let isDiscardingScan = $state(false);
  let errorHeading = $state("That scan didn’t start.");
  let errorGuidance = $state("Check the folder and try again.");
  let errorMessage = $state("");
  let scanStarted = $state(false);
  let scanActionError = $state("");
  let scanProgressAnnouncement = $state("");
  let scanAnnouncementCheckpoint: ScanProgressAnnouncementCheckpoint | null = null;
  let navigationError = $state("");
  let navigationErrorTitle = $state("That folder could not be opened.");
  let navigationRecovery = $state<NavigationRecovery | null>(null);
  let navigationRetryInFlight = $state(false);
  let revealError = $state("");
  let revealErrorNodeId = $state<number | null>(null);
  let revealRetryInFlight = $state(false);
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
  let scanDetailsOpen = $state(false);
  let landingHeading: HTMLHeadingElement | undefined = $state();
  let chooseDirectoryButton: HTMLButtonElement | null = $state(null);
  let scanProgressHeading: HTMLHeadingElement | undefined = $state();
  let viewHeading: HTMLHeadingElement | undefined = $state();
  let sunburstElement: SVGSVGElement | undefined = $state();
  let chartBackButton: HTMLButtonElement | null = $state(null);
  let chartFocusId: number | null = $state(null);
  let itemListElement: HTMLDivElement | undefined = $state();
  let listFocusId: number | null = $state(null);
  let listFocusAction: ListNavigationAction = "open";
  let preservingListFocusAction = false;
  let stateNotice: HTMLDivElement | undefined = $state();
  let scanActionNotice: HTMLDivElement | undefined = $state();
  let navigationNotice: HTMLDivElement | undefined = $state();
  let revealNotice: HTMLDivElement | undefined = $state();
  let allocatedMetricButton: HTMLButtonElement | undefined = $state();
  let logicalMetricButton: HTMLButtonElement | undefined = $state();
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
  const rootsPreviewStatus = import.meta.env.DEV
    ? scanRootsPreviewStatus(
        new URLSearchParams(window.location.search).get("roots"),
      )
    : null;
  const rootsPreview = rootsPreviewStatus !== null;
  const previewScanRoots: ScanRoot[] = [
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
  ];
  let dropActive = $state(dropPreview);
  let droppedPaths = $state<string[]>(
    dropPreview ? ["/Users/demo/Design Archive"] : [],
  );
  let preparingScanRoot = $state<string | null>(null);
  let scanRoots = $state<ScanRoot[]>(
    rootsPreviewStatus === "ready" ? previewScanRoots : [],
  );
  let scanRootsStatus = $state<ScanRootsStatus>(
    rootsPreviewStatus ?? "idle",
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
  // Deliberate pointer movement may temporarily supersede keyboard focus. When
  // the pointer leaves, focus resumes ownership before the clicked inspection.
  const activeEntry = $derived(pointerEntry ?? focusedEntry ?? inspectedEntry);
  const viewBytes = $derived(view ? metricBytes(view, sizeMetric) : 0);
  const parentId = $derived(view?.breadcrumbs.at(-2)?.id ?? null);
  const canEstimateSavings = $derived(
    inspectedEntry?.kind === "file" &&
      compressionState !== null &&
      ["notCompressed", "enabled", "disabled", "inherited"].includes(
        compressionState.state,
      ),
  );
  const inspectorStatus = $derived(
    inspectorAnnouncement({
      itemName: inspectedEntry?.name ?? null,
      isInspectingCompression,
      compressionLabel: compressionState
        ? formatCompressionState(compressionState)
        : null,
      isEstimatingSavings,
      isCancellingEstimate,
      estimateLabel: savingsEstimate
        ? formatSavingsEstimate(savingsEstimate)
        : null,
      hasEstimateStopError: estimateActionError.length > 0,
    }),
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
            ? `Top ${view.items.length} of ${view.totalItems - view.suppressedItems}`
            : `${view.totalItems - view.suppressedItems} ${view.totalItems - view.suppressedItems === 1 ? "item" : "items"}`
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
  ) {
    if (
      isResultBusy ||
      (listFocusAction === "reveal" && revealingNodeId !== null)
    ) return;
    const targetId = listNavigationTarget(visibleItemIds, itemId, event.key);
    if (targetId === null) return;

    event.preventDefault();
    listFocusId = targetId;
    await tick();
    const targetItem = visibleItems.find((item) => item.id === targetId);
    const targetAction = listNavigationActionTarget(
      listFocusAction,
      targetItem !== undefined && targetItem.kind !== "symlink",
    );
    const target = itemListElement?.querySelector<HTMLElement>(
      `[data-list-${targetAction}-id="${targetId}"]`,
    );
    preservingListFocusAction = true;
    target?.focus();
    preservingListFocusAction = false;
  }

  function handleListFocus(item: ScanItem, action: ListNavigationAction) {
    listFocusId = item.id;
    focusedEntry = item;
    pointerEntry = null;
    if (!preservingListFocusAction) listFocusAction = action;
  }

  function clearFocusedEntry(itemId: number | null) {
    if (itemId === null) return;
    if (focusedEntry?.id === itemId) focusedEntry = null;
  }

  function clearPointerEntry(itemId: number | null) {
    if (itemId === null) return;
    if (pointerEntry?.id === itemId) pointerEntry = null;
  }

  function clearDropState() {
    dropActive = false;
    droppedPaths = [];
  }

  function clearNavigationError() {
    navigationError = "";
    navigationRecovery = null;
  }

  function clearRevealError() {
    revealError = "";
    revealErrorNodeId = null;
  }

  async function showScanEntryError(
    title: string,
    detail: string,
    guidance: string,
    recovery: NavigationRecovery | null = null,
  ) {
    if (status === "complete" && result && view) {
      clearRevealError();
      navigationErrorTitle = title;
      navigationError = detail;
      navigationRecovery = recovery;
      await tick();
      navigationNotice?.focus();
    } else {
      status = "error";
      scanStarted = false;
      scanActionError = "";
      errorHeading = title;
      errorGuidance = guidance;
      errorMessage = detail;
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
      await showScanEntryError(
        "That folder can’t be scanned.",
        String(error),
        scanEntryRecoveryMessage("folder"),
      );
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

  async function retryScanRoots() {
    if (rootsPreviewStatus !== null) {
      scanRootsStatus = "loading";
      await new Promise((resolve) => setTimeout(resolve, 450));
      scanRoots = previewScanRoots;
      scanRootsStatus = "ready";
      return;
    }
    await loadScanRoots();
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
          "That drop did not contain exactly one folder.",
          scanEntryRecoveryMessage("drop"),
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
    clearNavigationError();
    clearRevealError();
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
      await showScanEntryError(
        "Folder picker didn’t open.",
        String(error),
        scanEntryRecoveryMessage("picker"),
        { kind: "chooseDirectory" },
      );
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
    errorGuidance = "Check the folder and try again.";
    errorMessage = "";
    clearNavigationError();
    navigationErrorTitle = "That folder could not be opened.";
    clearRevealError();
    revealingNodeId = null;
    sizeMetric = "allocated";
    compressionCapability = null;
    scanDetailsOpen = false;
    if (import.meta.env.DEV) {
      delete document.documentElement.dataset.cepaScanRenderMs;
    }
    progress = null;
    scanProgressAnnouncement = "";
    scanAnnouncementCheckpoint = null;
    result = null;
    view = null;
    pointerEntry = null;
    focusedEntry = null;
    clearInspection();
    resetDirectorySearch(true);
    scanId = null;

    await tick();
    scanProgressHeading?.focus();

    let resolveScan: (response: ScanResponse) => void;
    let rejectScan: (reason: string) => void;
    const completion = new Promise<ScanResponse>((resolve, reject) => {
      resolveScan = resolve;
      rejectScan = reject;
    });
    // A terminal channel event can beat the immediate command acknowledgement.
    // Observe rejection now; awaiting this same promise below still propagates it.
    void completion.catch(() => {});
    const onEvent = new Channel<ScanEvent>();
    onEvent.onmessage = (message) => {
      if (message.event === "started") {
        scanStarted = true;
        scanId = message.scanId;
      } else if (message.event === "progress") {
        scanStarted = true;
        scanId = message.scanId;
        progress = message.progress;
        publishScanProgressAnnouncement(
          message.progress,
          status === "cancelling",
        );
      } else if (message.event === "completed") {
        resolveScan(message.response);
      } else {
        rejectScan(message.message);
      }
    };

    try {
      const acknowledgedScanId = await invoke<number>("scan_directory", {
        path: requestedPath,
        onEvent,
      });
      scanStarted = true;
      scanId = acknowledgedScanId;
      const response = await completion;
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
        const presentation = scanFailurePresentation(scanStarted);
        errorHeading = presentation.heading;
        errorGuidance = presentation.guidance;
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

  function publishScanProgressAnnouncement(
    nextProgress: ScanProgress,
    cancelling: boolean,
  ) {
    const update = nextScanProgressAnnouncement(
      scanAnnouncementCheckpoint,
      nextProgress,
      cancelling,
    );
    if (!update) return;
    scanAnnouncementCheckpoint = update.checkpoint;
    scanProgressAnnouncement = update.announcement;
  }

  async function cancelScan() {
    if (scanId === null || status !== "scanning") return;
    const cancellingScanId = scanId;
    scanActionError = "";
    status = "cancelling";
    publishScanProgressAnnouncement(displayProgress, true);
    try {
      await invoke<boolean>("cancel_scan", { scanId: cancellingScanId });
    } catch (error) {
      if (scanId !== cancellingScanId || status !== "cancelling") return;
      status = "scanning";
      scanProgressAnnouncement = "";
      scanAnnouncementCheckpoint = {
        phase: displayProgress.phase,
        cancelling: false,
        announcedElapsedMs: Math.max(0, displayProgress.elapsedMs),
      };
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
      clearNavigationError();
      clearRevealError();
      isDiscardingScan = true;
      try {
        await invoke("discard_scan", { scanId: completedScanId });
      } catch (error) {
        if (scanId === completedScanId && status === "complete") {
          navigationErrorTitle = "This scan could not be closed.";
          navigationError = String(error);
          navigationRecovery = { kind: "discardScan" };
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
    errorGuidance = "Check the folder and try again.";
    scanId = null;
    progress = null;
    result = null;
    view = null;
    scanDetailsOpen = false;
    pointerEntry = null;
    focusedEntry = null;
    clearInspection();
    resetDirectorySearch(true);
    errorMessage = "";
    clearNavigationError();
    navigationErrorTitle = "That folder could not be opened.";
    clearRevealError();
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
      isCancellingEstimate ||
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
        navigateUpFromDesktopCommand();
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

  function navigateUpFromDesktopCommand() {
    if (parentId === null || isResultBusy) return;
    chartBackButton?.focus();
    void openDirectory(parentId);
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

  async function retryDirectorySearch() {
    const query = searchQuery.trim();
    if (!query || scanId === null || !view || isResultBusy) return;

    invalidateDirectorySearch();
    isSearching = true;
    const sequence = searchSequence;
    const completedScanId = scanId;
    const nodeId = view.nodeId;
    const metric = sizeMetric;
    await tick();
    searchInput?.focus();
    void runDirectorySearch(
      sequence,
      completedScanId,
      nodeId,
      metric,
      query,
    );
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
    preserveRecovery = false,
  ): Promise<boolean> {
    if (scanId === null || isResultBusy) return false;
    const completedScanId = scanId;
    const request = {
      scanId: completedScanId,
      sequence: ++navigationSequence,
    };
    const preservedSearch = focusHeading ? "" : searchQuery;
    if (focusHeading) resetDirectorySearch(true);
    else invalidateDirectorySearch();
    isNavigating = true;
    if (!preserveRecovery) clearNavigationError();
    invalidateRevealRequest();
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
      ) return false;
      clearNavigationError();
      view = nextView;
      sizeMetric = metric;
      pointerEntry = null;
      focusedEntry = null;
      clearInspection();
      if (preservedSearch.trim()) {
        searchQuery = preservedSearch;
        scheduleDirectorySearch();
      }
      if (focusHeading) {
        await tick();
        viewHeading?.focus();
      }
      return true;
    } catch (error) {
      if (
        !isCurrentCompletedScanRequest(request, {
          scanId,
          sequence: navigationSequence,
          complete: status === "complete",
        })
      ) return false;
      navigationErrorTitle = focusHeading
        ? "That folder could not be opened."
        : "The size metric could not be changed.";
      navigationError = String(error);
      navigationRecovery = focusHeading
        ? { kind: "directory", nodeId, metric, focusHeading: true }
        : { kind: "metric", nodeId, metric, focusHeading: false };
      await tick();
      navigationNotice?.focus();
      return false;
    } finally {
      if (navigationSequence === request.sequence) isNavigating = false;
    }
  }

  async function openDirectory(nodeId: number | null) {
    if (nodeId === null) return;
    await loadDirectory(nodeId, sizeMetric, true);
  }

  function focusSelectedMetricInDetails(event: Event) {
    event.preventDefault();
    requestAnimationFrame(() => {
      (sizeMetric === "allocated"
        ? allocatedMetricButton
        : logicalMetricButton
      )?.focus();
    });
  }

  async function setSizeMetric(metric: SizeMetric) {
    if (!view || metric === sizeMetric || isResultBusy) return;
    const succeeded = await loadDirectory(view.nodeId, metric, false);
    if (succeeded) return;
    scanDetailsOpen = false;
    await tick();
    navigationNotice?.focus();
  }

  async function retryNavigationAction() {
    const recovery = navigationRecovery;
    if (!recovery || isResultBusy) return;

    if (recovery.kind === "chooseDirectory") {
      await chooseDirectory();
      return;
    }
    if (recovery.kind === "discardScan") {
      await reset();
      return;
    }

    navigationRetryInFlight = true;
    let succeeded = false;
    try {
      succeeded = await loadDirectory(
        recovery.nodeId,
        recovery.metric,
        recovery.focusHeading,
        true,
      );
    } finally {
      navigationRetryInFlight = false;
    }
    if (!succeeded || recovery.focusHeading) return;
    scanDetailsOpen = true;
    await tick();
    (recovery.metric === "allocated"
      ? allocatedMetricButton
      : logicalMetricButton
    )?.focus();
  }

  async function revealItem(
    nodeId: number,
    preserveRecovery = false,
  ): Promise<boolean> {
    if (scanId === null || revealingNodeId !== null || isResultBusy) return false;
    const completedScanId = scanId;
    const request = {
      scanId: completedScanId,
      sequence: ++revealSequence,
    };
    revealingNodeId = nodeId;
    clearNavigationError();
    if (!preserveRecovery) clearRevealError();
    try {
      await invoke("reveal_scan_item", { scanId: completedScanId, nodeId });
      if (
        !isCurrentCompletedScanRequest(request, {
          scanId,
          sequence: revealSequence,
          complete: status === "complete",
        })
      ) return false;
      clearRevealError();
      return true;
    } catch (error) {
      if (
        !isCurrentCompletedScanRequest(request, {
          scanId,
          sequence: revealSequence,
          complete: status === "complete",
        })
      ) return false;
      revealError = String(error);
      revealErrorNodeId = nodeId;
      await tick();
      revealNotice?.focus();
      return false;
    } finally {
      if (revealSequence === request.sequence) revealingNodeId = null;
    }
  }

  async function retryReveal() {
    const nodeId = revealErrorNodeId;
    if (
      nodeId === null ||
      revealRetryInFlight ||
      revealingNodeId !== null ||
      isResultBusy
    ) return;
    revealRetryInFlight = true;
    let succeeded = false;
    try {
      succeeded = await revealItem(nodeId, true);
    } finally {
      revealRetryInFlight = false;
    }
    if (!succeeded) return;
    await tick();
    (
      itemListElement?.querySelector<HTMLElement>(
        `[data-list-reveal-id="${nodeId}"]`,
      ) ?? viewHeading
    )?.focus();
  }

  function invalidateRevealRequest() {
    revealSequence += 1;
    revealingNodeId = null;
    revealRetryInFlight = false;
    clearRevealError();
  }

  function invalidateCompletedScanRequests() {
    // Backend scan IDs authorize each command; these frontend generations also
    // prevent an older completion from mutating a newer UI lifecycle.
    navigationSequence += 1;
    isNavigating = false;
    navigationRetryInFlight = false;
    clearNavigationError();
    invalidateRevealRequest();
  }

  function activateEntry(
    entry: ChartItem | ScanItem,
    trigger?: HTMLElement | SVGGElement,
  ) {
    if (isResultBusy) return;
    if (entry.kind === "directory") {
      void openDirectory(entry.id);
    } else {
      inspectionReturnTarget = trigger ?? null;
      void inspectEntry(entry);
    }
  }

  function previewEntry(entry: ChartItem | ScanItem) {
    if (isResultBusy) return;
    if (pointerEntry?.id !== entry.id) pointerEntry = entry;
  }

  function handleSegmentKeydown(event: KeyboardEvent, entry: ChartItem) {
    if (isResultBusy) {
      if (chartInteractionKeys.has(event.key)) event.preventDefault();
      return;
    }
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

<Tooltip.Provider delayDuration={150}>
<WindowTitlebar inert={dropOverlayVisible} />
<div class="app-shell">
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
      class:landing-recovery={status === "error" || status === "cancelled"}
      inert={dropOverlayVisible}
      aria-hidden={dropOverlayVisible ? "true" : undefined}
    >
      <section class="landing-copy" aria-labelledby="landing-title">
        <div class="app-symbol" aria-hidden="true"><CepaMark /></div>
        <h1 id="landing-title" tabindex="-1" bind:this={landingHeading}>
          Find what’s taking up space.
        </h1>
        <p class="lede">
          Choose a disk or folder to see its largest files and subfolders.
        </p>

        <ScanRootPicker
          roots={scanRoots}
          status={scanRootsStatus}
          busy={isBusy}
          preparingPath={preparingScanRoot}
          onSelect={(rootPath) => void startValidatedRoot(rootPath, false)}
          onRetry={retryScanRoots}
          onRetryFocusFallback={() => chooseDirectoryButton?.focus()}
        />

        <div class="scan-entry">
          <Button
            class="choose-button"
            bind:ref={chooseDirectoryButton}
            variant={scanRoots.length > 0 ? "outline" : "default"}
            size="lg"
            disabled={isBusy}
            title={`Choose folder (${primaryShortcutLabel}O)`}
            onclick={chooseDirectory}
          >
            <FolderOpen data-icon="inline-start" />
            Choose folder…
          </Button>

          <details class="manual-path" inert={isBusy}>
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

          {#if status === "error"}
            <div
              class="error-callout"
              role="alert"
              tabindex="-1"
              bind:this={stateNotice}
            >
              <AlertCircle />
              <div>
                <strong>{errorHeading}</strong>
                <span>{errorGuidance}</span>
                {#if errorMessage}
                  <details class="metadata-disclosure state-error-details">
                    <summary>Error details</summary>
                    <p>{errorMessage}</p>
                  </details>
                {/if}
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
      <p
        class="sr-only scan-progress-announcement"
        aria-live="polite"
        aria-atomic="true"
      >{scanProgressAnnouncement}</p>
      <section class="scan-progress" aria-labelledby="scan-progress-title">
        <div class="scan-titlebar">
          <div>
            <span id="scan-progress-status" class="scan-kicker">
              <span class="status-dot" aria-hidden="true"></span>
              {progressPresentation.statusLabel}
            </span>
            <h1
              id="scan-progress-title"
              tabindex="-1"
              aria-describedby="scan-progress-status"
              bind:this={scanProgressHeading}
            >{scanTargetName}</h1>
          </div>
          <Button
            class="scan-stop-action"
            variant="outline"
            size="sm"
            onclick={cancelScan}
            aria-disabled={scanId === null || status === "cancelling"}
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
              <span>The scan is still running. Try stopping it again.</span>
              <details class="metadata-disclosure state-error-details">
                <summary>Error details</summary>
                <p>{scanActionError}</p>
              </details>
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
      </section>
    </main>
  {:else if result && view}
    {@const completedResult = result}
    <main
      class="results-view"
      inert={dropOverlayVisible}
      aria-hidden={dropOverlayVisible ? "true" : undefined}
    >
      <section class="results-heading">
        <div class="result-navigation">
          <Button
            class="result-home-action"
            variant="ghost"
            size="sm"
            aria-busy={isDiscardingScan}
            aria-disabled={isBusy}
            aria-label="Back to Cepa home"
            onclick={reset}
          >
            <ArrowLeft data-icon="inline-start" />
            Back
          </Button>
        </div>
        <div class="result-actions" role="group" aria-label="Analysis actions">
          {#if result.skippedEntries > 0}
            <Tooltip.Root>
              <Tooltip.Trigger
                class={buttonVariants({
                  variant: "ghost",
                  size: "icon-sm",
                  class: "coverage-warning-action",
                })}
                aria-label={`Some items weren’t included. ${formatUnavailableItems(result.skippedEntries)}, so totals may be lower than the space actually in use.`}
              >
                <TriangleAlert />
              </Tooltip.Trigger>
              <Tooltip.Content
                side="bottom"
                align="end"
                sideOffset={6}
                class="coverage-warning-tooltip"
              >
                <p>
                  <strong>Some items weren’t included.</strong>
                  <span>{formatUnavailableItems(result.skippedEntries)}, so totals may be lower than the space actually in use.</span>
                </p>
              </Tooltip.Content>
            </Tooltip.Root>
          {/if}
          <Popover.Root bind:open={scanDetailsOpen}>
            <Popover.Trigger
              class={buttonVariants({
                variant: "ghost",
                size: "icon-sm",
                class: "result-info-action",
              })}
              aria-label="Details"
              title="Details"
            >
              <Info />
            </Popover.Trigger>
            <Popover.Content
              align="end"
              sideOffset={6}
              onOpenAutoFocus={focusSelectedMetricInDetails}
              class="scan-details-popover w-80 max-w-[calc(100vw-2rem)] gap-0 overflow-hidden p-0"
            >
              <Popover.Header class="scan-details-header">
                <Popover.Title>DETAILS</Popover.Title>
              </Popover.Header>
              <dl class="scan-details-list">
                <div class="scan-details-metric">
                  <dt>Size basis</dt>
                  <dd>
                    <div class="metric-switch" role="group" aria-label="Size basis">
                      <button
                        type="button"
                        bind:this={allocatedMetricButton}
                        aria-pressed={sizeMetric === "allocated"}
                        aria-disabled={isResultBusy}
                        onclick={() => setSizeMetric("allocated")}
                      >On disk</button>
                      <button
                        type="button"
                        bind:this={logicalMetricButton}
                        aria-pressed={sizeMetric === "logical"}
                        aria-disabled={isResultBusy}
                        onclick={() => setSizeMetric("logical")}
                      >Logical</button>
                    </div>
                  </dd>
                </div>
                <div><dt>Files</dt><dd>{formatCount(result.fileCount)}</dd></div>
                <div><dt>Folders</dt><dd>{formatCount(result.directoryCount)}</dd></div>
                <div><dt>Scan time</dt><dd>{formatDuration(result.elapsedMs)}</dd></div>
                <div><dt>Scanner</dt><dd>{formatBackend(result.backend)}</dd></div>
                <div><dt>Space on disk</dt><dd>{result.allocatedSizeIsEstimate ? "Estimated" : "Exact"}</dd></div>
                <div><dt>Hard links</dt><dd>{result.hardLinkDeduplicationSupported ? "Counted once" : "Not deduplicated"}</dd></div>
                <div><dt>Other filesystems</dt><dd>{result.sameFilesystemEnforced ? "Not traversed" : "Boundary unavailable"}</dd></div>
                <div><dt>Items not included</dt><dd>{formatCount(result.skippedEntries)}</dd></div>
                <div><dt>Mounted filesystems skipped</dt><dd>{formatCount(result.skippedFilesystems)}</dd></div>
                {#if result.skippedCloudEntries > 0}
                  <div><dt>Cloud-only items skipped</dt><dd>{formatCount(result.skippedCloudEntries)}</dd></div>
                {/if}
                {#if result.duplicateHardLinks > 0}
                  <div><dt>Duplicate hard links</dt><dd>{formatCount(result.duplicateHardLinks)}</dd></div>
                {/if}
                {#if compressionCapability}
                  <div class="compression-detail">
                    <dt>Filesystem compression</dt>
                    <dd title={compressionCapability.detail}>
                      {compressionCapability.status === "inspectOnly"
                        ? "Available for analysis"
                        : compressionCapability.status === "unsupported"
                          ? "Not supported"
                          : "Couldn’t be checked"}
                    </dd>
                  </div>
                {/if}
              </dl>
            </Popover.Content>
          </Popover.Root>
        </div>
        <div class="result-title">
          <Tooltip.Root ignoreNonKeyboardFocus={true}>
            <Tooltip.Trigger tabindex={-1}>
              {#snippet child({ props: { type: _type, ...triggerProps } })}
                <span {...triggerProps} class="result-title-trigger">
                  <h1
                    tabindex="-1"
                    bind:this={resultHeading}
                    aria-label={`${completedResult.displayName}, ${completedResult.root}`}
                  >{completedResult.displayName}</h1>
                </span>
              {/snippet}
            </Tooltip.Trigger>
            <Tooltip.Content
              side="bottom"
              align="start"
              sideOffset={6}
              class="result-path-tooltip"
            >{completedResult.root}</Tooltip.Content>
          </Tooltip.Root>
        </div>
        <div class="result-total">
          <span>Space on disk{result.allocatedSizeIsEstimate ? " (estimated)" : ""}</span>
          <strong>{formatBytes(result.allocatedBytes)}</strong>
        </div>
      </section>

      <div class="explorer-toolbar">
        <nav class="breadcrumbs" aria-label="Current scan path">
          {#each view.breadcrumbs as breadcrumb, index (breadcrumb.id)}
            {#if index > 0}<ChevronRight aria-hidden="true" />{/if}
            <button
              type="button"
              aria-disabled={isResultBusy}
              aria-current={index === view.breadcrumbs.length - 1 ? "page" : undefined}
              onclick={() => openDirectory(breadcrumb.id)}
            >{breadcrumb.name}</button>
          {/each}
        </nav>
      </div>

      {#if navigationError}
        <div
          class="error-callout navigation-error"
          role="alert"
          tabindex="-1"
          bind:this={navigationNotice}
        >
          <AlertCircle />
          <div class="result-action-error-copy">
            <strong>{navigationErrorTitle}</strong>
            <span>{navigationRecoveryMessage(navigationRecovery)}</span>
            {#if navigationRecovery}
              <div class="result-action-error-actions">
                <Button
                  variant="outline"
                  size="xs"
                  aria-disabled={isResultBusy || navigationRetryInFlight}
                  onclick={retryNavigationAction}
                >{navigationRetryInFlight ? "Trying…" : "Try again"}</Button>
              </div>
            {/if}
            <details class="metadata-disclosure result-action-error-details">
              <summary>Error details</summary>
              <p>{navigationError}</p>
            </details>
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
          <div class="result-action-error-copy">
            <strong>Couldn’t show that item.</strong>
            <span>The current folder is unchanged. Try again when you’re ready.</span>
            <div class="result-action-error-actions">
              <Button
                variant="outline"
                size="xs"
                aria-disabled={isResultBusy || revealingNodeId !== null || revealRetryInFlight}
                onclick={retryReveal}
              >{revealRetryInFlight ? "Trying…" : "Try again"}</Button>
            </div>
            <details class="metadata-disclosure result-action-error-details">
              <summary>Error details</summary>
              <p>{revealError}</p>
            </details>
          </div>
        </div>
      {/if}

      <section
        class="explorer"
        aria-label="Storage map and folder contents"
        aria-busy={isResultBusy}
      >
        <div class="chart-pane">
          <h2 class="sr-only">
            Storage map for {view.displayName}
          </h2>
          {#if view.path !== view.root}
            <Button
              class="chart-back"
              variant="ghost"
              size="sm"
              bind:ref={chartBackButton}
              aria-disabled={isResultBusy}
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
                aria-busy={isResultBusy}
                aria-label={`Storage map for ${view.displayName} by ${formatMetric(sizeMetric).toLowerCase()}`}
                aria-describedby="sunburst-navigation-help"
                bind:this={sunburstElement}
              >
                {#each sunburstSegments as segment (segment.key)}
                  {#if segment.item.id !== null}
                    <g
                      role="button"
                      tabindex={chartFocusId === segment.item.id ? 0 : -1}
                      data-chart-node-id={segment.item.id}
                      aria-disabled={isResultBusy}
                      aria-label={`${segment.item.name}, ${formatBytes(metricBytes(segment.item, sizeMetric))}`}
                      onpointermove={() => previewEntry(segment.item)}
                      onfocus={() => {
                        chartFocusId = segment.item.id;
                        focusedEntry = segment.item;
                        pointerEntry = null;
                      }}
                      onmouseleave={() => clearPointerEntry(segment.item.id)}
                      onblur={() => clearFocusedEntry(segment.item.id)}
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

            <div class="chart-center" aria-hidden="true">
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
          <p
            class="sr-only inspector-status"
            aria-live="polite"
            aria-atomic="true"
          >{inspectorStatus}</p>
          <div class="section-heading">
            <h2 tabindex="-1" bind:this={viewHeading}>{view.displayName}</h2>
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
            >
              <header class="inspector-heading">
                <div class="inspector-heading-copy">
                  <span>{inspectedEntry.kind === "file" ? "File details" : "Item details"}</span>
                  <strong title={inspectedEntry.name}>{inspectedEntry.name}</strong>
                </div>
                <div class="inspector-actions">
                  {#if isEstimatingSavings}
                    <Button
                      class="estimate-cancel-action"
                      variant="outline"
                      size="xs"
                      aria-disabled={isCancellingEstimate}
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
                  <div class="estimate-readout estimate-readout-active">
                    <div>
                      <span>Potential savings</span>
                      <strong>{isCancellingEstimate ? "Stopping…" : "Estimating…"}</strong>
                    </div>
                    {#if estimateActionError}
                      <div
                        class="estimate-action-error"
                        role="alert"
                        tabindex="-1"
                        bind:this={estimateActionNotice}
                      >
                        <AlertCircle aria-hidden="true" />
                        <div class="estimate-action-error-copy">
                          <p>
                            <strong>Couldn’t stop estimating.</strong>
                            <span>The estimate is still running. Try again.</span>
                          </p>
                          <details class="metadata-disclosure estimate-action-error-details">
                            <summary>Error details</summary>
                            <p>{estimateActionError}</p>
                          </details>
                        </div>
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
              <strong>Couldn’t search this folder</strong>
              <span>Try again, or clear the search to return to this folder.</span>
              <div class="search-message-actions">
                <Button variant="outline" size="sm" onclick={retryDirectorySearch}>Try again</Button>
                <Button variant="ghost" size="sm" onclick={clearDirectorySearch}>Clear search</Button>
              </div>
              <details class="metadata-disclosure search-error-details">
                <summary>Search details</summary>
                <p>{searchError}</p>
              </details>
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
                    aria-disabled={isResultBusy}
                    onclick={(event) => activateEntry(item, event.currentTarget)}
                    onpointermove={() => previewEntry(item)}
                    onfocus={() => handleListFocus(item, "open")}
                    onmouseleave={() => clearPointerEntry(item.id)}
                    onblur={() => clearFocusedEntry(item.id)}
                    onkeydown={(event) => handleListNavigation(event, item.id)}
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
                      aria-disabled={isResultBusy || revealingNodeId !== null}
                      aria-label={`Reveal ${item.name} in the system file manager`}
                      title="Reveal in file manager"
                      onclick={() => revealItem(item.id)}
                      onpointermove={() => previewEntry(item)}
                      onfocus={() => handleListFocus(item, "reveal")}
                      onmouseleave={() => clearPointerEntry(item.id)}
                      onblur={() => clearFocusedEntry(item.id)}
                      onkeydown={(event) => handleListNavigation(event, item.id)}
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
              <strong>{view.suppressedItems > 0 ? "No items to show" : "This folder is empty"}</strong>
            </div>
          {/if}
        </div>
      </section>

    </main>
  {/if}
</div>
</Tooltip.Provider>
