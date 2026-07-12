<script lang="ts">
  import { Channel, invoke } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";
  import {
    AlertCircle,
    ArrowLeft,
    ChevronRight,
    Clock3,
    Database,
    File,
    Files,
    Folder,
    FolderOpen,
    Gauge,
    HardDrive,
    RotateCcw,
    ScanSearch,
    X,
  } from "@lucide/svelte";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import {
    formatBytes,
    formatCount,
    formatDuration,
    type ChartItem,
    type DirectoryView,
    type ScanEvent,
    type ScanItem,
    type ScanProgress,
    type ScanResponse,
    type ScanResult,
  } from "$lib/scanner";
  import { createSunburst } from "$lib/sunburst";

  type Status = "idle" | "scanning" | "cancelling" | "complete" | "error";

  let path = $state("");
  let status = $state<Status>("idle");
  let scanId = $state<number | null>(null);
  let progress = $state<ScanProgress | null>(null);
  let result = $state<ScanResult | null>(null);
  let view = $state<DirectoryView | null>(null);
  let selectedEntry = $state<ChartItem | ScanItem | null>(null);
  let isNavigating = $state(false);
  let errorMessage = $state("");

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
    },
  );
  const sunburstSegments = $derived(createSunburst(view?.chartItems ?? []));
  const parentPath = $derived(view?.breadcrumbs.at(-2)?.path ?? null);

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
    }
  }

  async function startScan(event?: SubmitEvent) {
    event?.preventDefault();
    const requestedPath = path.trim();
    if (!requestedPath || isBusy) return;

    status = "scanning";
    errorMessage = "";
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
    } catch (error) {
      const message = String(error);
      if (message.toLowerCase().includes("cancelled")) {
        status = "idle";
        errorMessage = "";
      } else {
        status = "error";
        errorMessage = message;
      }
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
  }

  function itemPercent(bytes: number): number {
    if (!view || view.allocatedBytes <= 0) return 0;
    return Math.max(0.8, Math.min(100, (bytes / view.allocatedBytes) * 100));
  }

  async function openDirectory(directoryPath: string | null) {
    if (scanId === null || !directoryPath || isNavigating) return;
    isNavigating = true;
    try {
      view = await invoke<DirectoryView>("open_scan_directory", {
        scanId,
        path: directoryPath,
      });
      selectedEntry = null;
    } catch (error) {
      errorMessage = `Could not open that folder: ${String(error)}`;
    } finally {
      isNavigating = false;
    }
  }

  function activateEntry(entry: ChartItem | ScanItem) {
    if (entry.kind === "directory") void openDirectory(entry.path);
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
      <span class="status-dot" data-active={isBusy}></span>
      <span>{isBusy ? "Portable scan active" : "Portable scanner"}</span>
      <span class="header-separator"></span>
      <span>jwalk</span>
    </div>
  </header>

  {#if status === "idle" || status === "error"}
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
            <div class="error-callout" role="alert">
              <AlertCircle />
              <div>
                <strong>That scan didn’t start.</strong>
                <span>{errorMessage}</span>
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
    <main class="scan-view" aria-live="polite">
      <section class="scan-heading">
        <div>
          <p class="eyebrow">{status === "cancelling" ? "Stopping safely" : "Reading the map"}</p>
          <h1>{formatBytes(displayProgress.allocatedBytes)}</h1>
          <p class="scan-path" title={displayProgress.currentPath}>
            {displayProgress.currentPath || path}
          </p>
        </div>
        <Button variant="outline" onclick={cancelScan} disabled={status === "cancelling"}>
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

      <div class="scan-pulse" aria-hidden="true">
        <div class="pulse-disc"><ScanSearch /></div>
        <span class="pulse-ring ring-one"></span>
        <span class="pulse-ring ring-two"></span>
        <span class="pulse-ring ring-three"></span>
      </div>

      <p class="scan-footnote">
        Following no links · Staying on this filesystem · Counting hard links once
      </p>
    </main>
  {:else if result && view}
    <main class="results-view">
      <section class="results-heading">
        <div>
          <p class="eyebrow">Scan complete</p>
          <h1>{result.displayName}</h1>
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

      <nav class="breadcrumbs" aria-label="Current scan path">
        {#each view.breadcrumbs as breadcrumb, index (breadcrumb.path)}
          {#if index > 0}<ChevronRight aria-hidden="true" />{/if}
          <button
            type="button"
            aria-current={index === view.breadcrumbs.length - 1 ? "page" : undefined}
            onclick={() => openDirectory(breadcrumb.path)}
          >{breadcrumb.name}</button>
        {/each}
      </nav>

      <section class="explorer" aria-label="Storage map and folder contents">
        <div class="chart-pane">
          <div class="chart-heading">
            <div>
              <p class="eyebrow">Storage map</p>
              <h2>{view.displayName}</h2>
            </div>
            {#if view.path !== view.root}
              <Button
                variant="ghost"
                size="sm"
                onclick={() => openDirectory(parentPath)}
              >
                <ArrowLeft data-icon="inline-start" /> Up
              </Button>
            {/if}
          </div>

          <div class="sunburst-wrap">
            {#if sunburstSegments.length > 0}
              <svg class="sunburst" viewBox="0 0 340 340" role="img" aria-label={`Storage map for ${view.displayName}`}>
                {#each sunburstSegments as segment (`${segment.item.path ?? segment.item.name}-${segment.depth}`)}
                  {#if segment.item.path}
                    <g
                      role="button"
                      tabindex="0"
                      aria-label={`${segment.item.name}, ${formatBytes(segment.item.allocatedBytes)}`}
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
                        data-selected={selectedEntry?.path === segment.item.path}
                      />
                    </g>
                  {:else}
                    <path
                      d={segment.pathData}
                      data-depth={segment.depth}
                      data-color={segment.colorIndex}
                      data-selected={selectedEntry?.path === segment.item.path}
                    />
                  {/if}
                {/each}
              </svg>
            {:else}
              <div class="chart-empty"><Folder /></div>
            {/if}

            <div class="chart-center" aria-live="polite">
              <span>{selectedEntry ? "Selected" : "This folder"}</span>
              <strong>{formatBytes(selectedEntry?.allocatedBytes ?? view.allocatedBytes)}</strong>
              <em>{selectedEntry?.name ?? view.displayName}</em>
            </div>
          </div>

          <p class="chart-help">Select a ring or row to inspect · Open a folder to drill down</p>
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
              {#each view.items as item, index (item.path)}
                <button
                  type="button"
                  class="storage-item"
                  data-selected={selectedEntry?.path === item.path}
                  onclick={() => activateEntry(item)}
                  onmouseenter={() => (selectedEntry = item)}
                  onfocus={() => (selectedEntry = item)}
                  onmouseleave={() => (selectedEntry = null)}
                  onblur={() => (selectedEntry = null)}
                  aria-label={`${item.name}, ${formatBytes(item.allocatedBytes)}${item.kind === "directory" ? ", open folder" : ""}`}
                >
                  <span class="item-index">{String(index + 1).padStart(2, "0")}</span>
                  <span class="item-icon" data-kind={item.kind}>
                    {#if item.kind === "directory"}<Folder />{:else}<File />{/if}
                  </span>
                  <span class="item-copy">
                    <strong title={item.name}>{item.name}</strong>
                    <span>
                      {formatCount(item.fileCount)} files · {formatCount(item.directoryCount)} folders
                    </span>
                    <span class="item-bar" aria-hidden="true">
                      <span style:width={`${itemPercent(item.allocatedBytes)}%`}></span>
                    </span>
                  </span>
                  <span class="item-size">
                    <strong>{formatBytes(item.allocatedBytes)}</strong>
                    <span>{((item.allocatedBytes / Math.max(view.allocatedBytes, 1)) * 100).toFixed(1)}%</span>
                  </span>
                  {#if item.kind === "directory"}<ChevronRight class="item-chevron" />{/if}
                </button>
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

      {#if result.skippedEntries > 0 || result.skippedFilesystems > 0 || result.duplicateHardLinks > 0}
        <footer class="result-notes">
          <span>Accounting notes</span>
          {#if result.skippedEntries > 0}<p>{result.skippedEntries} unreadable entries skipped</p>{/if}
          {#if result.skippedFilesystems > 0}<p>{result.skippedFilesystems} mounted filesystems not traversed</p>{/if}
          {#if result.duplicateHardLinks > 0}<p>{result.duplicateHardLinks} duplicate hard links counted once</p>{/if}
        </footer>
      {/if}
    </main>
  {/if}
</div>
