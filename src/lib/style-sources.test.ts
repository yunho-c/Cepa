import { describe, expect, test } from "bun:test";

describe("production style sources", () => {
  test("scan only the frontend source tree for Tailwind utilities", async () => {
    const stylesheet = await Bun.file(new URL("../app.css", import.meta.url)).text();

    expect(stylesheet).toContain('@import "tailwindcss" source(none);');
    expect(stylesheet.match(/^@source .+;$/gm)).toEqual(['@source ".";']);
  });

  test("keeps inspector copy rules from overriding the action row", async () => {
    const [component, stylesheet] = await Promise.all([
      Bun.file(new URL("../App.svelte", import.meta.url)).text(),
      Bun.file(new URL("../app.css", import.meta.url)).text(),
    ]);

    expect(component).toContain('class="inspector-heading-copy"');
    expect(stylesheet).toContain(".inspector-heading-copy {");
    expect(stylesheet).not.toContain(".inspector-heading > div {");
  });

  test("centers the storage landing content without repeating the privacy line", async () => {
    const [component, stylesheet] = await Promise.all([
      Bun.file(new URL("../App.svelte", import.meta.url)).text(),
      Bun.file(new URL("../app.css", import.meta.url)).text(),
    ]);
    const storageLandingStart = stylesheet.indexOf(".landing-with-storage {");
    const storageLanding = stylesheet.slice(
      storageLandingStart,
      stylesheet.indexOf("}", storageLandingStart),
    );

    expect(component).toContain(
      "Choose a disk or folder to see its largest files and subfolders.",
    );
    expect(component).not.toContain("Everything stays on this device.");
    expect(storageLanding).toContain("align-items: safe center;");
    expect(storageLanding).toContain("justify-items: center;");
    expect(storageLanding).not.toContain("place-items: start center;");
  });

  test("keeps pending explorer actions focused with guarded aria-disabled state", async () => {
    const component = await Bun.file(new URL("../App.svelte", import.meta.url)).text();
    const storageStart = component.indexOf('class="storage-item"');
    const revealStart = component.indexOf('class="reveal-item"');
    const storageButton = component.slice(
      storageStart,
      component.indexOf("</button>", storageStart),
    );
    const revealButton = component.slice(
      revealStart,
      component.indexOf("</button>", revealStart),
    );

    expect(storageStart).toBeGreaterThan(-1);
    expect(revealStart).toBeGreaterThan(-1);
    expect(storageButton).toContain("aria-disabled={isResultBusy}");
    expect(storageButton).not.toMatch(/^\s*disabled=/m);
    expect(revealButton).toContain(
      "aria-disabled={isResultBusy || revealingNodeId !== null}",
    );
    expect(revealButton).not.toMatch(/^\s*disabled=/m);
  });

  test("keeps pending cancellation actions focused with guarded aria-disabled state", async () => {
    const component = await Bun.file(new URL("../App.svelte", import.meta.url)).text();
    const stopStart = component.indexOf('class="scan-stop-action"');
    const estimateCancelStart = component.indexOf('class="estimate-cancel-action"');
    const stopButton = component.slice(stopStart, component.indexOf("</Button>", stopStart));
    const estimateCancelButton = component.slice(
      estimateCancelStart,
      component.indexOf("</Button>", estimateCancelStart),
    );

    expect(stopStart).toBeGreaterThan(-1);
    expect(estimateCancelStart).toBeGreaterThan(-1);
    expect(stopButton).toContain(
      'aria-disabled={scanId === null || status === "cancelling"}',
    );
    expect(stopButton).not.toMatch(/^\s*disabled=/m);
    expect(estimateCancelButton).toContain("aria-disabled={isCancellingEstimate}");
    expect(estimateCancelButton).not.toMatch(/^\s*disabled=/m);
    expect(component).toContain('if (scanId === null || status !== "scanning") return;');
    expect(component).toMatch(
      /status = "scanning";\s+scanProgressAnnouncement = "";\s+scanAnnouncementCheckpoint = \{/,
    );
    expect(component).toMatch(
      /!isEstimatingSavings \|\|\s+isCancellingEstimate \|\|\s+isDiscardingScan/,
    );
  });

  test("keeps the radial navigator owned and inert while navigation is pending", async () => {
    const [component, stylesheet] = await Promise.all([
      Bun.file(new URL("../App.svelte", import.meta.url)).text(),
      Bun.file(new URL("../app.css", import.meta.url)).text(),
    ]);
    const segmentStart = component.indexOf('role="button"', component.indexOf('class="sunburst"'));
    const segment = component.slice(segmentStart, component.indexOf("</g>", segmentStart));
    const activateStart = component.indexOf("function activateEntry(");
    const activateEntry = component.slice(
      activateStart,
      component.indexOf("function previewEntry(", activateStart),
    );
    const previewStart = component.indexOf("function previewEntry(");
    const previewEntry = component.slice(
      previewStart,
      component.indexOf("function handleSegmentKeydown(", previewStart),
    );
    const keyboardStart = component.indexOf("function handleSegmentKeydown(");
    const segmentKeyboard = component.slice(
      keyboardStart,
      component.indexOf("</script>", keyboardStart),
    );

    expect(segmentStart).toBeGreaterThan(-1);
    expect(segment).toContain("aria-disabled={isResultBusy}");
    expect(component).toContain("aria-busy={isResultBusy}");
    expect(activateEntry).toContain("if (isResultBusy) return;");
    expect(previewEntry).toContain("if (isResultBusy) return;");
    expect(segmentKeyboard).toContain(
      "if (chartInteractionKeys.has(event.key)) event.preventDefault();",
    );
    expect(stylesheet).toContain('.sunburst[aria-busy="true"] { opacity: 0.45; }');
    expect(stylesheet).toContain('.sunburst g[aria-disabled="true"] { cursor: wait; }');
  });

  test("keeps result Back focused while its snapshot is released", async () => {
    const [component, stylesheet] = await Promise.all([
      Bun.file(new URL("../App.svelte", import.meta.url)).text(),
      Bun.file(new URL("../app.css", import.meta.url)).text(),
    ]);
    const homeStart = component.indexOf('class="result-home-action"');
    const homeButton = component.slice(homeStart, component.indexOf("</Button>", homeStart));
    const resetStart = component.indexOf("async function reset(");
    const reset = component.slice(resetStart, component.indexOf("async function", resetStart + 1));

    expect(homeStart).toBeGreaterThan(-1);
    expect(homeButton).toContain('aria-label="Back to Cepa home"');
    expect(homeButton).toContain("aria-busy={isDiscardingScan}");
    expect(homeButton).toContain("aria-disabled={isBusy}");
    expect(homeButton).not.toMatch(/^\s*disabled=/m);
    expect(reset).toContain("if (isBusy) return;");
    expect(stylesheet).toContain('.result-home-action[aria-disabled="true"] { cursor: wait; }');
    expect(stylesheet).not.toContain(".result-home-action:disabled");
  });

  test("connects the result Info action to the quiet scan details popover", async () => {
    const component = await Bun.file(new URL("../App.svelte", import.meta.url)).text();
    const infoStart = component.indexOf("<Popover.Trigger");
    const infoButton = component.slice(infoStart, component.indexOf("</Popover.Trigger>", infoStart));
    const detailsStart = component.indexOf("<Popover.Content");
    const details = component.slice(
      detailsStart,
      component.indexOf("</Popover.Content>", detailsStart),
    );
    const toolbarStart = component.indexOf('class="explorer-toolbar"');
    const toolbar = component.slice(
      toolbarStart,
      component.indexOf("</div>", toolbarStart),
    );
    const metricStart = component.indexOf("async function setSizeMetric(");
    const setSizeMetric = component.slice(
      metricStart,
      component.indexOf("async function retryNavigationAction(", metricStart),
    );
    const retryStart = component.indexOf("async function retryNavigationAction(");
    const retryNavigation = component.slice(
      retryStart,
      component.indexOf("async function revealItem(", retryStart),
    );

    expect(infoStart).toBeGreaterThan(-1);
    expect(infoButton).toContain('variant: "ghost"');
    expect(infoButton).toContain('size: "icon-sm"');
    expect(infoButton).toContain('class: "result-info-action"');
    expect(infoButton).toContain('aria-label="Details"');
    expect(detailsStart).toBeGreaterThan(-1);
    expect(component).toContain("bind:open={scanDetailsOpen}");
    expect(component).toContain('align="end"');
    expect(component).toContain("onOpenAutoFocus={focusSelectedMetricInDetails}");
    expect(component).toContain('class="scan-details-popover');
    expect(component).toContain("<Popover.Title>DETAILS</Popover.Title>");
    expect(details).toContain("<dt>Size basis</dt>");
    expect(details).toContain('class="metric-switch" role="group" aria-label="Size basis"');
    expect(details).toContain("aria-disabled={isResultBusy}");
    expect(toolbar).not.toContain("metric-switch");
    expect(setSizeMetric).toContain("const succeeded = await loadDirectory(");
    expect(setSizeMetric).toContain("if (succeeded) return;");
    expect(setSizeMetric.indexOf("if (succeeded) return;")).toBeLessThan(
      setSizeMetric.indexOf("scanDetailsOpen = false;"),
    );
    expect(setSizeMetric).toContain("navigationNotice?.focus();");
    expect(retryNavigation).toContain("scanDetailsOpen = true;");
    expect(component).toContain("<dt>Files</dt><dd>{formatCount(result.fileCount)}</dd>");
    expect(component).toContain("<dt>Folders</dt><dd>{formatCount(result.directoryCount)}</dd>");
    expect(component).toContain("<dt>Scan time</dt><dd>{formatDuration(result.elapsedMs)}</dd>");
    expect(component).not.toContain('class="result-summary"');
    expect(component).not.toContain("Technical information about this completed scan.");
    expect(component).not.toContain('class="scan-details"');
    expect(component).not.toContain("<details id=\"scan-details\"");
  });

  test("moves incomplete coverage into a hover and focus warning action", async () => {
    const [component, stylesheet] = await Promise.all([
      Bun.file(new URL("../App.svelte", import.meta.url)).text(),
      Bun.file(new URL("../app.css", import.meta.url)).text(),
    ]);
    const warningStart = component.indexOf("<Tooltip.Trigger");
    const warningTrigger = component.slice(
      warningStart,
      component.indexOf("</Tooltip.Trigger>", warningStart),
    );
    const tooltipStart = component.indexOf("<Tooltip.Content", warningStart);
    const tooltip = component.slice(
      tooltipStart,
      component.indexOf("</Tooltip.Content>", tooltipStart),
    );

    expect(component).toContain("<Tooltip.Provider delayDuration={150}>");
    expect(component).toContain("{#if result.skippedEntries > 0}");
    expect(warningStart).toBeGreaterThan(-1);
    expect(warningTrigger).toContain('class: "coverage-warning-action"');
    expect(warningTrigger).toContain("formatUnavailableItems(result.skippedEntries)");
    expect(tooltip).toContain("Some items weren’t included.");
    expect(tooltip).toContain("totals may be lower than the space actually in use.");
    expect(tooltip).not.toContain("aria-live");
    expect(tooltip).not.toContain('role="status"');
    expect(tooltip).not.toContain('role="alert"');
    expect(component).not.toContain('class="coverage-notice"');
    expect(stylesheet).toContain('.coverage-warning-action[data-slot="tooltip-trigger"]');
    expect(stylesheet).not.toContain(".coverage-notice");
  });

  test("keeps the primary explorer flat and fluid while separating its pane surfaces", async () => {
    const stylesheet = await Bun.file(new URL("../app.css", import.meta.url)).text();
    const resultsStart = stylesheet.indexOf(".results-view {");
    const results = stylesheet.slice(
      resultsStart,
      stylesheet.indexOf("}", resultsStart),
    );
    const explorerStart = stylesheet.indexOf(".explorer {");
    const explorer = stylesheet.slice(
      explorerStart,
      stylesheet.indexOf("}", explorerStart),
    );
    const chartPaneStart = stylesheet.indexOf(".chart-pane {");
    const chartPane = stylesheet.slice(
      chartPaneStart,
      stylesheet.indexOf("}", chartPaneStart),
    );
    const directoryPaneStart = stylesheet.indexOf(".directory-pane {");
    const directoryPane = stylesheet.slice(
      directoryPaneStart,
      stylesheet.indexOf("}", directoryPaneStart),
    );

    expect(resultsStart).toBeGreaterThan(-1);
    expect(results).toContain("width: 100%");
    expect(results).toContain("max-width: none");
    expect(results).not.toContain("max-width: 1240px");
    expect(explorerStart).toBeGreaterThan(-1);
    expect(stylesheet).toContain("--content-height: calc(100vh - var(--titlebar-height))");
    expect(results).toContain("height: var(--content-height)");
    expect(results).toContain("display: flex");
    expect(results).toContain("flex-direction: column");
    expect(explorer).toContain("flex: 1");
    expect(explorer).toContain("min-height: 214px");
    expect(explorer).toContain(
      "grid-template-columns: clamp(320px, 32vw, 520px) minmax(0, 1fr)",
    );
    expect(explorer).not.toContain("590px");
    expect(explorer).not.toContain("border:");
    expect(explorer).not.toContain("border-radius:");
    expect(explorer).not.toContain("box-shadow:");
    expect(explorer).not.toContain("background:");
    expect(chartPane).toContain("border-right: 1px solid var(--border)");
    expect(chartPane).toContain("background: var(--quiet-surface)");
    expect(directoryPane).toContain("background: var(--card)");
    expect(stylesheet).not.toContain(".explorer { border-radius:");
    expect(stylesheet).not.toContain(".result-summary");
  });

  test("moves desktop Up focus to its stable pending control", async () => {
    const component = await Bun.file(new URL("../App.svelte", import.meta.url)).text();
    const commandStart = component.indexOf("function runDesktopCommand(");
    const commandHandler = component.slice(
      commandStart,
      component.indexOf("function handleDesktopKeydown(", commandStart),
    );
    const backStart = component.indexOf('class="chart-back"');
    const backButton = component.slice(backStart, component.indexOf("</Button>", backStart));

    expect(commandHandler).toContain('case "navigateUp":');
    expect(commandHandler).toContain("navigateUpFromDesktopCommand();");
    expect(commandHandler).toContain("if (parentId === null || isResultBusy) return;");
    expect(commandHandler).toContain("chartBackButton?.focus();");
    expect(commandHandler).toContain("void openDirectory(parentId);");
    expect(backStart).toBeGreaterThan(-1);
    expect(backButton).toContain("bind:ref={chartBackButton}");
    expect(backButton).toContain("aria-disabled={isResultBusy}");
    expect(backButton).not.toMatch(/^\s*disabled=/m);
  });

  test("keeps the visual chart preview out of live announcements", async () => {
    const component = await Bun.file(new URL("../App.svelte", import.meta.url)).text();
    const centerStart = component.indexOf('class="chart-center"');
    const chartCenter = component.slice(centerStart, component.indexOf("</div>", centerStart));

    expect(centerStart).toBeGreaterThan(-1);
    expect(chartCenter).toContain('aria-hidden="true"');
    expect(chartCenter).not.toContain("aria-live");
    expect(component).toContain('role="group"');
    expect(component).toContain('aria-describedby="sunburst-navigation-help"');
    expect(component).toContain(
      'aria-label={`${segment.item.name}, ${formatBytes(metricBytes(segment.item, sizeMetric))}`}',
    );
  });

  test("moves the completed root path into the result title tooltip", async () => {
    const [component, stylesheet] = await Promise.all([
      Bun.file(new URL("../App.svelte", import.meta.url)).text(),
      Bun.file(new URL("../app.css", import.meta.url)).text(),
    ]);
    const titleStart = component.indexOf('class="result-title"');
    const title = component.slice(
      titleStart,
      component.indexOf('class="result-total"', titleStart),
    );

    expect(titleStart).toBeGreaterThan(-1);
    expect(title).toContain("<Tooltip.Root ignoreNonKeyboardFocus={true}>");
    expect(title).toContain('class="result-title-trigger"');
    expect(title).toContain(
      'aria-label={`${completedResult.displayName}, ${completedResult.root}`}',
    );
    expect(title).toContain('class="result-path-tooltip"');
    expect(title).toContain(">{completedResult.root}</Tooltip.Content>");
    expect(title).not.toContain('class="result-path"');
    expect(stylesheet).toContain(
      '.result-path-tooltip[data-slot="tooltip-content"]',
    );
    expect(stylesheet).toContain("overflow-wrap: anywhere");
    expect(stylesheet).not.toContain(".result-path {");
  });

  test("coordinates list-driven selection with sunburst emphasis", async () => {
    const [component, stylesheet] = await Promise.all([
      Bun.file(new URL("../App.svelte", import.meta.url)).text(),
      Bun.file(new URL("../app.css", import.meta.url)).text(),
    ]);

    expect(component).toContain(
      "data-selected={activeEntry?.id === segment.item.id}",
    );
    expect(component).toContain("data-selected={activeEntry?.id === item.id}");
    expect(stylesheet).toContain(
      '.sunburst:has(path[data-selected="true"]) path:not([data-selected="true"]) { opacity: 0.36; }',
    );
    expect(stylesheet).not.toContain(".sunburst:has(g:hover");
  });

  test("scopes inspector announcements to one atomic status sentence", async () => {
    const component = await Bun.file(new URL("../App.svelte", import.meta.url)).text();
    const statusStart = component.indexOf('class="sr-only inspector-status"');
    const status = component.slice(statusStart, component.indexOf("</p>", statusStart));
    const inspectorStart = component.indexOf('class="selection-inspector"');
    const inspector = component.slice(
      inspectorStart,
      component.indexOf("</section>", inspectorStart),
    );

    expect(statusStart).toBeGreaterThan(-1);
    expect(status).toContain('aria-live="polite"');
    expect(status).toContain('aria-atomic="true"');
    expect(inspectorStart).toBeGreaterThan(-1);
    expect(inspector).not.toContain("aria-live");
    expect(inspector).toContain('role="alert"');
  });

  test("hands successful directory navigation to the visible folder heading", async () => {
    const component = await Bun.file(new URL("../App.svelte", import.meta.url)).text();
    const chartHeadingStart = component.indexOf('<h2 class="sr-only">');
    const chartHeading = component.slice(
      chartHeadingStart,
      component.indexOf("</h2>", chartHeadingStart),
    );
    const directoryHeadingStart = component.indexOf(
      '<h2 tabindex="-1" bind:this={viewHeading}>',
    );
    const directoryHeading = component.slice(
      directoryHeadingStart,
      component.indexOf("</h2>", directoryHeadingStart),
    );

    expect(chartHeadingStart).toBeGreaterThan(-1);
    expect(chartHeading).not.toContain("viewHeading");
    expect(chartHeading).not.toContain("tabindex");
    expect(directoryHeadingStart).toBeGreaterThan(-1);
    expect(directoryHeading).toContain("{view.displayName}");
  });

  test("keeps scan details popover out of live announcements", async () => {
    const component = await Bun.file(new URL("../App.svelte", import.meta.url)).text();
    const detailsStart = component.indexOf('class="scan-details-popover');
    const details = component.slice(
      detailsStart,
      component.indexOf("</Popover.Content>", detailsStart),
    );

    expect(detailsStart).toBeGreaterThan(-1);
    expect(details).toContain("Filesystem compression");
    expect(details).not.toContain("aria-live");
    expect(details).not.toContain('role="status"');
    expect(details).not.toContain('role="alert"');
  });

  test("describes scan-start focus without announcing placeholder progress", async () => {
    const component = await Bun.file(new URL("../App.svelte", import.meta.url)).text();
    const statusStart = component.indexOf('id="scan-progress-status"');
    const headingStart = component.indexOf('id="scan-progress-title"');
    const heading = component.slice(
      headingStart,
      component.indexOf("</h1>", headingStart),
    );
    const announcementStart = component.indexOf(
      'class="sr-only scan-progress-announcement"',
    );
    const announcement = component.slice(
      announcementStart,
      component.indexOf("</p>", announcementStart),
    );

    expect(statusStart).toBeGreaterThan(-1);
    expect(headingStart).toBeGreaterThan(-1);
    expect(heading).toContain('aria-describedby="scan-progress-status"');
    expect(announcementStart).toBeGreaterThan(-1);
    expect(announcement).toContain('aria-live="polite"');
    expect(announcement).toContain('aria-atomic="true"');
    expect(announcement).toContain("{scanProgressAnnouncement}");
    expect(announcement).not.toContain("{progressPresentation.announcement}");
  });

  test("keeps the active scan focused on progress instead of partial results", async () => {
    const [component, stylesheet] = await Promise.all([
      Bun.file(new URL("../App.svelte", import.meta.url)).text(),
      Bun.file(new URL("../app.css", import.meta.url)).text(),
    ]);

    expect(component).not.toContain("Largest so far");
    expect(component).not.toContain("Largest files observed so far");
    expect(component).not.toContain("Looking for files…");
    expect(component).not.toContain("ScanSearch");
    expect(stylesheet).not.toContain(".partial-results");
    expect(stylesheet).not.toContain(".scan-empty-progress");
    expect(stylesheet).toMatch(
      /\.scan-view \{[^}]*display: grid;\s+align-items: safe center;/,
    );
  });

  test("keeps long scan paths from widening the active scan grid", async () => {
    const stylesheet = await Bun.file(new URL("../app.css", import.meta.url)).text();

    expect(stylesheet).toContain(".scan-progress { width: 100%; min-width: 0; }");
    expect(stylesheet).toContain(
      `.scan-path { color: var(--muted-foreground); font: 11px/1.5 ui-monospace, "SFMono-Regular", Consolas, monospace; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }`,
    );
  });
});
