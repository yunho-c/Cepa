<script lang="ts">
  import { onDestroy, type Snippet } from "svelte";
  import { explorerSplit, explorerSplitKey } from "$lib/explorer-layout";
  import ChartPaneMenu from "$lib/components/chart-pane-menu.svelte";

  let { chart, directory, busy = false, showPercentage = $bindable(false) }: {
    chart: Snippet;
    directory: Snippet;
    busy?: boolean;
    showPercentage?: boolean;
  } = $props();

  const id = $props.id();
  let explorer: HTMLElement;
  let divider: HTMLDivElement;
  let containerWidth = $state(0);
  let viewportWidth = $state(window.innerWidth);
  let preferredWidth = $state<number | null>(null);
  let drag = $state<{
    pointerId: number;
    startX: number;
    startWidth: number;
    previousWidth: number | null;
    containerWidth: number;
    viewportWidth: number;
  } | null>(null);
  let pendingX = 0;
  let frame: number | null = null;
  const split = $derived(explorerSplit(containerWidth, viewportWidth, preferredWidth));
  const disabled = $derived(busy || split.stacked || split.min === split.max);
  const percent = (width: number) => split.available > 0
    ? Math.round(width / split.available * 100)
    : 0;

  $effect(() => {
    if (!explorer || containerWidth === 0) return;
    // Set a single CSS property through CSSOM; do not inject a stylesheet or
    // style attribute that would weaken the packaged WebView's CSP.
    if (split.stacked) explorer.style.removeProperty("--chart-width");
    else explorer.style.setProperty("--chart-width", `${split.width}px`);
  });

  $effect(() => {
    if (drag && (disabled || containerWidth !== drag.containerWidth
      || viewportWidth !== drag.viewportWidth)) cancelDrag();
    if (split.stacked && document.activeElement === divider) explorer?.focus();
  });

  function cancelFrame() {
    if (frame !== null) cancelAnimationFrame(frame);
    frame = null;
  }

  function releaseDrag() {
    cancelFrame();
    const pointerId = drag?.pointerId;
    drag = null;
    if (pointerId !== undefined && divider?.hasPointerCapture(pointerId)) {
      divider.releasePointerCapture(pointerId);
    }
  }

  function cancelDrag() {
    if (!drag) return;
    preferredWidth = drag.previousWidth;
    releaseDrag();
  }

  function applyPointer() {
    frame = null;
    if (!drag) return;
    // A pointer-up can beat the ResizeObserver or window binding update.
    if (explorer.clientWidth !== drag.containerWidth
      || window.innerWidth !== drag.viewportWidth) {
      cancelDrag();
      return;
    }
    preferredWidth = Math.min(split.max, Math.max(split.min,
      drag.startWidth + pendingX - drag.startX));
  }

  function startDrag(event: PointerEvent) {
    if (disabled || drag || event.button !== 0 || !event.isPrimary) return;
    event.preventDefault();
    divider.focus({ preventScroll: true });
    divider.setPointerCapture(event.pointerId);
    drag = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startWidth: split.width,
      previousWidth: preferredWidth,
      containerWidth,
      viewportWidth,
    };
    pendingX = event.clientX;
  }

  function moveDrag(event: PointerEvent) {
    if (drag?.pointerId !== event.pointerId) return;
    pendingX = event.clientX;
    if (frame === null) frame = requestAnimationFrame(applyPointer);
  }

  function finishDrag(event: PointerEvent) {
    if (drag?.pointerId !== event.pointerId) return;
    cancelFrame();
    pendingX = event.clientX;
    applyPointer();
    releaseDrag();
  }

  function cancelPointer(event: PointerEvent) {
    if (drag?.pointerId === event.pointerId) cancelDrag();
  }

  function reset() {
    if (disabled) return;
    releaseDrag();
    preferredWidth = null;
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.defaultPrevented || event.isComposing) return;
    if (drag && event.key === "Escape") {
      event.preventDefault();
      cancelDrag();
      return;
    }
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    const width = explorerSplitKey(event.key, split, event.shiftKey);
    if (width === undefined) return;
    event.preventDefault();
    if (disabled || drag) return;
    preferredWidth = width;
  }

  onDestroy(releaseDrag);
</script>

<svelte:window bind:innerWidth={viewportWidth} onblur={cancelDrag} />

<section
  class="explorer"
  aria-label="Storage map and folder contents"
  aria-busy={busy}
  data-resizing={drag !== null}
  bind:this={explorer}
  bind:clientWidth={containerWidth}
  tabindex="-1"
>
  <ChartPaneMenu id={`${id}-chart`} {busy} bind:showPercentage>
    {@render chart()}
  </ChartPaneMenu>
  <!-- A focusable separator is the WAI-ARIA window splitter pattern. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="explorer-divider"
    bind:this={divider}
    role="separator"
    tabindex="0"
    aria-label="Storage map width"
    aria-orientation="vertical"
    aria-controls={`${id}-chart`}
    aria-describedby={`${id}-resize-help`}
    aria-valuemin={percent(split.min)}
    aria-valuemax={percent(split.max)}
    aria-valuenow={percent(split.width)}
    aria-valuetext={`${Math.round(split.width)} pixels for storage map`}
    aria-disabled={disabled}
    title="Drag to resize. Double-click to reset."
    onpointerdown={startDrag}
    onpointermove={moveDrag}
    onpointerup={finishDrag}
    onpointercancel={cancelPointer}
    onlostpointercapture={cancelPointer}
    onkeydown={handleKeydown}
    onblur={() => {
      // Hiding a focused divider can blur it before the window binding updates.
      if (window.innerWidth < 560) explorer.focus({ preventScroll: true });
    }}
    ondblclick={reset}
  ></div>
  <div class="directory-pane">
    {@render directory()}
  </div>
  <p id={`${id}-resize-help`} class="sr-only">
    Use Left and Right Arrow keys to resize the storage map. Hold Shift for larger steps.
    Home and End move to the limits. Press Enter or double-click to reset.
    Press Escape to cancel a drag.
  </p>
</section>
