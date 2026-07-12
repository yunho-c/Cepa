<script lang="ts">
  import { Channel, invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import { tick } from "svelte";
  import {
    AlertCircle,
    ArrowLeft,
    ChevronRight,
    CircleStop,
    Clock3,
    Database,
    File,
    Files,
    Folder,
    FolderOpen,
    FolderSearch,
    Gauge,
    HardDrive,
    Link2,
    RotateCcw,
    ScanSearch,
    X,
  } from "@lucide/svelte";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import {
    formatBytes,
    formatBackend,
    formatCompressionCapability,
    formatCount,
    formatDuration,
    formatMetric,
    formatPercent,
    describeEntry,
    isCancellationError,
    metricBytes,
    type ChartItem,
    type CompressionCapability,
    type DirectoryView,
    type ScanEvent,
    type ScanItem,
    type ScanProgress,
    type ScanResponse,
    type ScanResult,
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
  let isNavigating = $state(false);
  let errorMessage = $state("");
  let navigationError = $state("");
  let navigationErrorTitle = $state("That folder could not be opened.");
  let revealError = $state("");
  let revealingNodeId = $state<number | null>(null);
  let sizeMetric = $state<SizeMetric>("allocated");
  let compressionCapability = $state<CompressionCapability | null>(null);
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
  const viewBytes = $derived(view ? metricBytes(view, sizeMetric) : 0);
  const parentId = $derived(view?.breadcrumbs.at(-2)?.id ?? null);
  const headerLabel = $derived(
    status === "scanning"
      ? "Scan active"
      : status === "cancelling"
        ? "Stopping scan"
        : status === "complete"
          ? "Scan complete"
          : status === "cancelled"
            ? "Scan stopped"
            : status === "error"
              ? "Needs attention"
              : "Ready",
  );
  const headerDetail = $derived(
    status === "complete" && result ? formatBackend(result.backend) : "Local only",
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

    <div class="header-meta">
      <span class="status-dot" data-state={status}></span>
      <span>{headerLabel}</span>
      <span class="header-separator"></span>
      <span class="header-detail">
        {headerDetail}
        {#if status === "complete" && result}<span class="header-technical"> · {result.backend}</span>{/if}
      </span>
    </div>
  </header>

  {#if status === "idle" || status === "error" || status === "cancelled"}
    <main class="landing">
      <section class="landing-copy" aria-labelledby="landing-title">
        <p class="eyebrow">Storage, without the archaeology.</p>
        <h1 id="landing-title">See where your<br />space went.</h1>
        <p class="lede">
          Scan a folder locally. Cepa maps the files that matter, without
          uploading a byte.
        </p>

        <div class="scan-entry">
          <Button class="choose-button" size="lg" onclick={chooseDirectory}>
            <FolderOpen data-icon="inline-start" />
            Choose a folder
          </Button>

          <div class="or-rule"><span>or enter a path</span></div>

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
              <ScanSearch data-icon="inline-end" />
            </Button>
          </form>

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
                  Kept your path after {formatCount(displayProgress.entriesScanned)} entries ·
                  {formatBytes(displayProgress.allocatedBytes)} observed
                </span>
              </div>
            </div>
          {/if}
        </div>
      </section>

      <aside class="landing-visual" aria-hidden="true">
        <div class="orbit orbit-outer"></div>
        <div class="orbit orbit-middle"></div>
        <div class="orbit orbit-inner"></div>
        <div class="visual-core">
          <HardDrive />
          <span>LOCAL</span>
        </div>
        <span class="orbit-label label-a">FILES</span>
        <span class="orbit-label label-b">SPACE</span>
        <span class="orbit-label label-c">CLEAR</span>
      </aside>
    </main>
  {:else if status === "scanning" || status === "cancelling"}
    <main class="scan-view" aria-busy="true">
      <p class="sr-only" aria-live="polite">{scanAnnouncement}</p>
      <section class="scan-heading">
        <div>
          <p class="eyebrow">{status === "cancelling" ? "Stopping safely" : "Reading the map"}</p>
          <h1>{formatBytes(displayProgress.allocatedBytes)}</h1>
          <p class="scan-path" title={displayProgress.currentPath}>
            {displayProgress.currentPath || path}
          </p>
        </div>
        <Button
          variant="outline"
          onclick={cancelScan}
          disabled={scanId === null || status === "cancelling"}
        >
          <X data-icon="inline-start" />
          {status === "cancelling" ? "Stopping…" : "Cancel"}
        </Button>
      </section>

      <div class="scan-line"><span></span></div>

      <section class="live-grid" aria-label="Live scan statistics">
        <div class="live-stat">
          <Files />
          <span>Entries</span>
          <strong>{formatCount(displayProgress.entriesScanned)}</strong>
        </div>
        <div class="live-stat">
          <File />
          <span>Files</span>
          <strong>{formatCount(displayProgress.filesScanned)}</strong>
        </div>
        <div class="live-stat">
          <Folder />
          <span>Folders</span>
          <strong>{formatCount(displayProgress.directoriesScanned)}</strong>
        </div>
        <div class="live-stat">
          <Clock3 />
          <span>Elapsed</span>
          <strong>{formatDuration(displayProgress.elapsedMs)}</strong>
        </div>
      </section>

      {#if displayProgress.largestItems.length > 0}
        <section class="partial-results" aria-label="Largest files observed so far">
          <div class="partial-heading">
            <span>Largest files so far</span>
            <span>Observed space on disk</span>
          </div>
          <ol>
            {#each displayProgress.largestItems.slice(0, 4) as item (item.id)}
              <li>
                <span class="partial-kind">File</span>
                <strong title={item.name}>{item.name}</strong>
                <span>{formatBytes(item.allocatedBytes)}</span>
              </li>
            {/each}
          </ol>
        </section>
      {:else}
        <div class="scan-pulse" aria-hidden="true">
          <div class="pulse-disc"><ScanSearch /></div>
          <span class="pulse-ring ring-one"></span>
          <span class="pulse-ring ring-two"></span>
          <span class="pulse-ring ring-three"></span>
        </div>
      {/if}

      <p class="scan-footnote">
        Symlinks stay closed · Mount boundaries are respected where available · Nothing leaves this device
      </p>
    </main>
  {:else if result && view}
    <main class="results-view">
      <section class="results-heading">
        <div>
          <div class="eyebrow-row">
            <p class="eyebrow">Scan complete</p>
            <span class="backend-badge" title={result.backend}>{formatBackend(result.backend)}</span>
          </div>
          <h1 tabindex="-1" bind:this={resultHeading}>{result.displayName}</h1>
          <p class="result-path" title={result.root}>{result.root}</p>
        </div>
        <div class="result-actions">
          <Button variant="outline" onclick={reset}>
            <RotateCcw data-icon="inline-start" />
            New scan
          </Button>
          <Button onclick={chooseDirectory}>
            <FolderOpen data-icon="inline-start" />
            Scan another
          </Button>
        </div>
      </section>

      <section class="summary-grid" aria-label="Scan summary">
        <div class="total-stat">
          <span>Space on disk {result.allocatedSizeIsEstimate ? "(estimate)" : ""}</span>
          <strong>{formatBytes(result.allocatedBytes)}</strong>
        </div>
        <div class="summary-stat">
          <Database />
          <span>Logical size</span>
          <strong>{formatBytes(result.logicalBytes)}</strong>
        </div>
        <div class="summary-stat">
          <Files />
          <span>Files</span>
          <strong>{formatCount(result.fileCount)}</strong>
        </div>
        <div class="summary-stat">
          <Folder />
          <span>Folders</span>
          <strong>{formatCount(result.directoryCount)}</strong>
        </div>
        <div class="summary-stat">
          <Gauge />
          <span>Scan time</span>
          <strong>{formatDuration(result.elapsedMs)}</strong>
        </div>
      </section>

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
          <span>Measure</span>
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
            <strong>That item could not be revealed.</strong>
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
          <div class="chart-heading">
            <div>
              <p class="eyebrow">Storage map</p>
              <h2 tabindex="-1" bind:this={viewHeading}>{view.displayName}</h2>
            </div>
            {#if view.path !== view.root}
              <Button
                variant="ghost"
                size="sm"
                disabled={isNavigating}
                onclick={() => openDirectory(parentId)}
              >
                <ArrowLeft data-icon="inline-start" /> Up
              </Button>
            {/if}
          </div>

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
                        data-selected={selectedEntry?.id === segment.item.id}
                      />
                    </g>
                  {:else}
                    <path
                      d={segment.pathData}
                      data-depth={segment.depth}
                      data-color={segment.colorIndex}
                      data-selected={selectedEntry?.id === segment.item.id}
                    />
                  {/if}
                {/each}
              </svg>
            {:else}
              <div class="chart-empty"><Folder /></div>
            {/if}

            <div class="chart-center" aria-live="polite">
              <span>{formatMetric(sizeMetric)}</span>
              <strong>{formatBytes(selectedEntry ? metricBytes(selectedEntry, sizeMetric) : viewBytes)}</strong>
              <em>{selectedEntry?.name ?? view.displayName}</em>
            </div>
          </div>

          <p class="chart-help">Ranked by {formatMetric(sizeMetric).toLowerCase()} · Select to inspect · Open folders to drill down</p>
        </div>

        <div class="directory-pane">
          <div class="section-heading">
            <div>
              <p class="eyebrow">Current folder</p>
              <h2>Largest first</h2>
            </div>
            <span>
              {view.itemsTruncated ? `Top ${view.items.length} of ${view.totalItems}` : `${view.totalItems} items`}
            </span>
          </div>

          {#if view.items.length > 0}
            <div class="item-list" class:is-navigating={isNavigating}>
              {#each view.items as item, index (item.id)}
                <div
                  class="storage-row"
                  data-selected={selectedEntry?.id === item.id}
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
                    aria-label={`${item.name}, ${formatBytes(metricBytes(item, sizeMetric))}${item.kind === "directory" ? ", open folder" : ""}`}
                  >
                    <span class="item-index">{String(index + 1).padStart(2, "0")}</span>
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
              <strong>This folder is empty.</strong>
              <span>There is nothing to account for here.</span>
            </div>
          {/if}
        </div>
      </section>

      <footer class="result-notes">
        <span>Accounting</span>
        <p>{formatBackend(result.backend)} ({result.backend})</p>
        <p>Allocated size {result.allocatedSizeIsEstimate ? "estimated" : "exact"}</p>
        <p>
          {result.hardLinkDeduplicationSupported
            ? "Hard links deduplicated"
            : "Hard-link deduplication unavailable"}
        </p>
        <p>
          {result.sameFilesystemEnforced
            ? "Filesystem boundary enforced"
            : "Filesystem boundary unavailable"}
        </p>
        {#if result.skippedEntries > 0}<p>{formatCount(result.skippedEntries)} unreadable entries skipped</p>{/if}
        {#if result.skippedFilesystems > 0}<p>{formatCount(result.skippedFilesystems)} mounted filesystems not traversed</p>{/if}
        {#if result.duplicateHardLinks > 0}<p>{formatCount(result.duplicateHardLinks)} duplicate hard links counted once</p>{/if}
        {#if compressionCapability}
          <p
            class="compression-note"
            data-status={compressionCapability.status}
            title={compressionCapability.detail}
            aria-live="polite"
          >{formatCompressionCapability(compressionCapability)}</p>
        {/if}
      </footer>
    </main>
  {/if}
</div>
