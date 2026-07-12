<script lang="ts">
  import { Channel, invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { tick } from "svelte";
  import {
    AlertCircle,
    ArrowLeft,
    ChevronRight,
    CircleStop,
    File,
    Folder,
    FolderOpen,
    FolderSearch,
    Link2,
    ScanSearch,
    X,
  } from "@lucide/svelte";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import {
    formatBytes,
    formatBackend,
    formatCompressionState,
    formatCount,
    formatDuration,
    formatMetric,
    formatPercent,
    formatSavingsEstimate,
    describeEntry,
    isCancellationError,
    metricBytes,
    type ChartItem,
    type CompressionCapability,
    type CompressionState,
    type DirectoryView,
    type ScanEvent,
    type ScanItem,
    type ScanProgress,
    type ScanResponse,
    type ScanResult,
    type SavingsEstimate,
    type SizeMetric,
  } from "$lib/scanner";
  import { createSunburst } from "$lib/sunburst";

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
  let errorMessage = $state("");
  let navigationError = $state("");
  let navigationErrorTitle = $state("That folder could not be opened.");
  let revealError = $state("");
  let revealingNodeId = $state<number | null>(null);
  let sizeMetric = $state<SizeMetric>("allocated");
  let compressionCapability = $state<CompressionCapability | null>(null);
  let compressionState = $state<CompressionState | null>(null);
  let isInspectingCompression = $state(false);
  let inspectionSequence = 0;
  let savingsEstimate = $state<SavingsEstimate | null>(null);
  let isEstimatingSavings = $state(false);
  let isCancellingEstimate = $state(false);
  let activeEstimateRequestId = $state<number | null>(null);
  let estimateRequestSequence = 0;
  let estimateActionButton: HTMLButtonElement | null = $state(null);
  let estimateCancelButton: HTMLButtonElement | null = $state(null);
  let resultHeading: HTMLHeadingElement | undefined = $state();
  let viewHeading: HTMLHeadingElement | undefined = $state();
  let stateNotice: HTMLDivElement | undefined = $state();
  let navigationNotice: HTMLDivElement | undefined = $state();
  let revealNotice: HTMLDivElement | undefined = $state();

  const isBusy = $derived(status === "scanning" || status === "cancelling");
  const displayProgress = $derived(
    progress ?? {
      entriesScanned: 0,
      filesScanned: 0,
      directoriesScanned: 0,
      logicalBytes: 0,
      allocatedBytes: 0,
      skippedEntries: 0,
      currentPath: path,
      elapsedMs: 0,
      largestItems: [],
    },
  );
  const sunburstSegments = $derived(createSunburst(view?.chartItems ?? [], sizeMetric));
  const activeEntry = $derived(inspectedEntry ?? selectedEntry);
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
  const scanAnnouncement = $derived(
    status === "cancelling"
      ? `Stopping after ${formatCount(displayProgress.entriesScanned)} entries.`
      : `Scanned ${formatCount(displayProgress.entriesScanned)} entries and ${formatBytes(displayProgress.allocatedBytes)}.`,
  );

  async function chooseDirectory() {
    errorMessage = "";
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
      errorMessage = `Could not open the folder picker: ${String(error)}`;
      status = "error";
      await tick();
      stateNotice?.focus();
    }
  }

  async function startScan(event?: SubmitEvent) {
    event?.preventDefault();
    const requestedPath = path.trim();
    if (!requestedPath || isBusy) return;

    status = "scanning";
    path = requestedPath;
    errorMessage = "";
    navigationError = "";
    navigationErrorTitle = "That folder could not be opened.";
    revealError = "";
    revealingNodeId = null;
    sizeMetric = "allocated";
    compressionCapability = null;
    progress = null;
    result = null;
    view = null;
    selectedEntry = null;
    clearInspection();
    scanId = null;

    const onEvent = new Channel<ScanEvent>();
    onEvent.onmessage = (message) => {
      if (message.event === "started") {
        scanId = message.scanId;
      } else if (message.event === "progress") {
        scanId = message.scanId;
        progress = message.progress;
      }
    };

    try {
      const response = await invoke<ScanResponse>("scan_directory", {
        path: requestedPath,
        onEvent,
      });
      scanId = response.scanId;
      result = response.result;
      view = response.view;
      path = response.result.root;
      status = "complete";
      await tick();
      resultHeading?.focus();
      void loadCompressionCapability(response.scanId);
    } catch (error) {
      const message = String(error);
      if (isCancellationError(message)) {
        status = "cancelled";
        scanId = null;
        errorMessage = "";
      } else {
        status = "error";
        scanId = null;
        errorMessage = message;
      }
      await tick();
      stateNotice?.focus();
    }
  }

  async function cancelScan() {
    if (scanId === null || !isBusy) return;
    status = "cancelling";
    try {
      await invoke("cancel_scan", { scanId });
    } catch (error) {
      status = "error";
      errorMessage = `Could not cancel the scan: ${String(error)}`;
      await tick();
      stateNotice?.focus();
    }
  }

  function reset() {
    status = "idle";
    scanId = null;
    progress = null;
    result = null;
    view = null;
    selectedEntry = null;
    clearInspection();
    errorMessage = "";
    navigationError = "";
    navigationErrorTitle = "That folder could not be opened.";
    revealError = "";
    revealingNodeId = null;
    sizeMetric = "allocated";
    compressionCapability = null;
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
  }

  async function inspectEntry(entry: ChartItem | ScanItem) {
    if (scanId === null || entry.id === null) return;
    clearEstimate();
    const completedScanId = scanId;
    const request = ++inspectionSequence;
    inspectedEntry = entry;
    compressionState = null;
    isInspectingCompression = true;
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
          detail: `The state request failed: ${String(error)}`,
        };
      }
    } finally {
      if (inspectionSequence === request) {
        isInspectingCompression = false;
      }
    }
  }

  function clearEstimate() {
    if (activeEstimateRequestId !== null && isEstimatingSavings) {
      void invoke("cancel_compression_estimate", {
        requestId: activeEstimateRequestId,
      });
    }
    savingsEstimate = null;
    isEstimatingSavings = false;
    isCancellingEstimate = false;
    activeEstimateRequestId = null;
  }

  async function estimateSavings() {
    if (
      scanId === null ||
      inspectedEntry === null ||
      inspectedEntry.id === null ||
      isEstimatingSavings
    ) return;
    const completedScanId = scanId;
    const nodeId = inspectedEntry.id;
    const requestId = ++estimateRequestSequence;
    activeEstimateRequestId = requestId;
    savingsEstimate = null;
    isEstimatingSavings = true;
    isCancellingEstimate = false;
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
          detail: `The estimate request failed: ${String(error)}`,
        };
      }
    } finally {
      if (activeEstimateRequestId === requestId) {
        isEstimatingSavings = false;
        isCancellingEstimate = false;
        activeEstimateRequestId = null;
        await tick();
        estimateActionButton?.focus();
      }
    }
  }

  async function cancelEstimate() {
    if (activeEstimateRequestId === null || !isEstimatingSavings) return;
    isCancellingEstimate = true;
    try {
      await invoke("cancel_compression_estimate", {
        requestId: activeEstimateRequestId,
      });
    } catch (error) {
      isCancellingEstimate = false;
      savingsEstimate = {
        status: "unavailable",
        algorithm: null,
        fidelity: "none",
        confidence: "none",
        sampledBytes: 0,
        logicalBytes: inspectedEntry?.logicalBytes ?? 0,
        allocatedBytes: inspectedEntry?.allocatedBytes ?? 0,
        estimatedSavingsLower: null,
        estimatedSavingsUpper: null,
        estimatorVersion: 1,
        detail: `The estimate could not be cancelled: ${String(error)}`,
      };
    }
  }

  function itemPercent(bytes: number): number {
    if (bytes <= 0 || viewBytes <= 0) return 0;
    return Math.max(0.8, Math.min(100, (bytes / viewBytes) * 100));
  }

  async function loadDirectory(
    nodeId: number,
    metric: SizeMetric,
    focusHeading: boolean,
  ) {
    if (scanId === null || isNavigating) return;
    isNavigating = true;
    navigationError = "";
    revealError = "";
    try {
      const nextView = await invoke<DirectoryView>("open_scan_directory", {
        scanId,
        nodeId,
        metric,
      });
      view = nextView;
      sizeMetric = metric;
      selectedEntry = null;
      clearInspection();
      if (focusHeading) {
        await tick();
        viewHeading?.focus();
      }
    } catch (error) {
      navigationErrorTitle = focusHeading
        ? "That folder could not be opened."
        : "The size metric could not be changed.";
      navigationError = String(error);
      await tick();
      navigationNotice?.focus();
    } finally {
      isNavigating = false;
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
    if (scanId === null || revealingNodeId !== null) return;
    revealingNodeId = nodeId;
    revealError = "";
    try {
      await invoke("reveal_scan_item", { scanId, nodeId });
    } catch (error) {
      revealError = String(error);
      await tick();
      revealNotice?.focus();
    } finally {
      revealingNodeId = null;
    }
  }

  function activateEntry(entry: ChartItem | ScanItem) {
    if (entry.kind === "directory") {
      void openDirectory(entry.id);
    } else {
      selectedEntry = entry;
      void inspectEntry(entry);
    }
  }

  function handleSegmentKeydown(event: KeyboardEvent, entry: ChartItem) {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      activateEntry(entry);
    }
  }
</script>

<svelte:head>
  <title>Cepa — Disk space, clearly</title>
  <meta
    name="description"
    content="Cepa is a fast, focused disk space analyzer for your desktop."
  />
</svelte:head>

<div class="app-shell">
  <header class="app-header">
    <button class="wordmark" type="button" onclick={reset} aria-label="Cepa home">
      <span class="wordmark-mark" aria-hidden="true">
        <span></span><span></span><span></span>
      </span>
      <span>Cepa</span>
    </button>

    {#if status === "complete"}
      <Button variant="outline" size="sm" onclick={chooseDirectory}>
        <FolderOpen data-icon="inline-start" />
        Choose folder
      </Button>
    {/if}
  </header>

  {#if status === "idle" || status === "error" || status === "cancelled"}
    <main class="landing">
      <section class="landing-copy" aria-labelledby="landing-title">
        <div class="app-symbol" aria-hidden="true"><ScanSearch /></div>
        <h1 id="landing-title">Find what’s taking up space.</h1>
        <p class="lede">
          Choose a folder to see its largest files and subfolders. Everything
          stays on this device.
        </p>

        <div class="scan-entry">
          <Button class="choose-button" size="lg" onclick={chooseDirectory}>
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
                placeholder="/Users/you/Documents"
                autocomplete="off"
                spellcheck={false}
                bind:value={path}
              />
              <Button variant="outline" type="submit" disabled={!path.trim()}>
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
                <strong>That scan didn’t start.</strong>
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
    <main class="scan-view" aria-busy="true">
      <p class="sr-only" aria-live="polite">{scanAnnouncement}</p>
      <section class="scan-card">
        <div class="scan-titlebar">
          <div>
            <span class="scan-kicker">
              <span class="status-dot" aria-hidden="true"></span>
              {status === "cancelling" ? "Stopping" : "Scanning"}
            </span>
            <h1>{scanTargetName}</h1>
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

        <div class="scan-total">
          <strong>{formatBytes(displayProgress.allocatedBytes)}</strong>
          <span>found so far</span>
        </div>
        <p class="scan-path" title={displayProgress.currentPath}>
          {displayProgress.currentPath || path}
        </p>

        <div class="scan-line" aria-hidden="true"><span></span></div>

        <dl class="scan-stats" aria-label="Scan progress">
          <div><dt>Items</dt><dd>{formatCount(displayProgress.entriesScanned)}</dd></div>
          <div><dt>Files</dt><dd>{formatCount(displayProgress.filesScanned)}</dd></div>
          <div><dt>Folders</dt><dd>{formatCount(displayProgress.directoriesScanned)}</dd></div>
          <div><dt>Elapsed</dt><dd>{formatDuration(displayProgress.elapsedMs)}</dd></div>
        </dl>

        {#if displayProgress.largestItems.length > 0}
          <section class="partial-results" aria-label="Largest files observed so far">
            <h2>Largest files found</h2>
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

        <p class="scan-footnote">Nothing leaves this device.</p>
      </section>
    </main>
  {:else if result && view}
    <main class="results-view">
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

      <div class="explorer-toolbar">
        <nav class="breadcrumbs" aria-label="Current scan path">
          {#each view.breadcrumbs as breadcrumb, index (breadcrumb.id)}
            {#if index > 0}<ChevronRight aria-hidden="true" />{/if}
            <button
              type="button"
              disabled={isNavigating}
              aria-current={index === view.breadcrumbs.length - 1 ? "page" : undefined}
              onclick={() => openDirectory(breadcrumb.id)}
            >{breadcrumb.name}</button>
          {/each}
        </nav>
        <div class="metric-switch" role="group" aria-label="Size metric">
          <button
            type="button"
            aria-pressed={sizeMetric === "allocated"}
            aria-disabled={isNavigating}
            onclick={() => setSizeMetric("allocated")}
          >On disk</button>
          <button
            type="button"
            aria-pressed={sizeMetric === "logical"}
            aria-disabled={isNavigating}
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
        aria-busy={isNavigating}
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
              disabled={isNavigating}
              onclick={() => openDirectory(parentId)}
            >
              <ArrowLeft data-icon="inline-start" /> Up
            </Button>
          {/if}

          <div class="sunburst-wrap">
            {#if sunburstSegments.length > 0}
              <svg class="sunburst" viewBox="0 0 340 340" role="img" aria-label={`Storage map for ${view.displayName} by ${formatMetric(sizeMetric).toLowerCase()}`}>
                {#each sunburstSegments as segment (`${segment.item.id ?? segment.item.name}-${segment.depth}`)}
                  {#if segment.item.id !== null}
                    <g
                      role="button"
                      tabindex="0"
                      aria-label={`${segment.item.name}, ${formatBytes(metricBytes(segment.item, sizeMetric))}`}
                      onmouseenter={() => (selectedEntry = segment.item)}
                      onfocus={() => (selectedEntry = segment.item)}
                      onmouseleave={() => (selectedEntry = null)}
                      onblur={() => (selectedEntry = null)}
                      onclick={() => activateEntry(segment.item)}
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

          <p class="chart-help">Select a segment to explore it</p>
        </div>

        <div class="directory-pane">
          <div class="section-heading">
            <h2>{view.displayName}</h2>
            <span>
              {view.itemsTruncated ? `Top ${view.items.length} of ${view.totalItems}` : `${view.totalItems} items`}
            </span>
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
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label="Close item details"
                  title="Close"
                  onclick={clearInspection}
                ><X /></Button>
              </header>
              <div class="inspection-state">
                <span>Compression</span>
                <strong data-state={compressionState?.state ?? "loading"}>
                  {isInspectingCompression
                    ? "Checking…"
                    : compressionState
                      ? formatCompressionState(compressionState)
                      : "Unavailable"}
                </strong>
              </div>
              {#if isEstimatingSavings}
                <div class="estimate-readout" aria-live="polite">
                  <div>
                    <span>Potential savings</span>
                    <strong>{isCancellingEstimate ? "Stopping…" : "Estimating…"}</strong>
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={isCancellingEstimate}
                    bind:ref={estimateCancelButton}
                    onclick={cancelEstimate}
                  >
                    Cancel
                  </Button>
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
            </section>
          {/if}

          {#if view.items.length > 0}
            <div class="item-list" class:is-navigating={isNavigating}>
              {#each view.items as item (item.id)}
                <div
                  class="storage-row"
                  data-selected={activeEntry?.id === item.id}
                >
                  <button
                    type="button"
                    class="storage-item"
                    disabled={isNavigating}
                    onclick={() => activateEntry(item)}
                    onmouseenter={() => (selectedEntry = item)}
                    onfocus={() => (selectedEntry = item)}
                    onmouseleave={() => (selectedEntry = null)}
                    onblur={() => (selectedEntry = null)}
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
                      <span class="item-bar" aria-hidden="true">
                        <span style:width={`${itemPercent(metricBytes(item, sizeMetric))}%`}></span>
                      </span>
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
                      disabled={isNavigating || revealingNodeId !== null}
                      aria-label={`Reveal ${item.name} in the system file manager`}
                      title="Reveal in file manager"
                      onclick={() => revealItem(item.id)}
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
          <span>
            <strong>Scan details</strong>
            <small>Scanner, accounting, and skipped items</small>
          </span>
          <ChevronRight aria-hidden="true" />
        </summary>
        <dl>
          <div><dt>Scanner</dt><dd>{formatBackend(result.backend)}</dd></div>
          <div><dt>Space on disk</dt><dd>{result.allocatedSizeIsEstimate ? "Estimated" : "Exact"}</dd></div>
          <div><dt>Hard links</dt><dd>{result.hardLinkDeduplicationSupported ? "Counted once" : "Not deduplicated"}</dd></div>
          <div><dt>Other filesystems</dt><dd>{result.sameFilesystemEnforced ? "Not traversed" : "Boundary unavailable"}</dd></div>
          <div><dt>Unreadable items</dt><dd>{formatCount(result.skippedEntries)}</dd></div>
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
