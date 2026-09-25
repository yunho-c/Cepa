<script lang="ts">
  import type { Snippet } from "svelte";
  import * as ContextMenu from "$lib/components/ui/context-menu";

  let { id, children, busy = false, showPercentage = $bindable(false) }: {
    id: string;
    children: Snippet;
    busy?: boolean;
    showPercentage?: boolean;
  } = $props();

  let open = $state(false);
  let pane: HTMLElement | null = $state(null);
  let returnTarget: HTMLElement | SVGElement | null = null;

  $effect(() => {
    if (busy && open) open = false;
  });

  function prepareMenu(event: MouseEvent) {
    if (busy) {
      event.preventDefault();
      return;
    }
    const target = event.target instanceof Element
      ? event.target.closest<HTMLElement | SVGElement>("[data-chart-node-id], button")
      : null;
    returnTarget = target ?? pane?.querySelector<SVGElement>('[data-chart-node-id][tabindex="0"]') ?? pane;
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key !== "ContextMenu" && !(event.key === "F10" && event.shiftKey)) return;
    if (event.altKey || event.ctrlKey || event.metaKey) return;
    event.preventDefault();
    if (busy || event.repeat || !(event.target instanceof Element)) return;
    const rect = event.target.getBoundingClientRect();
    event.target.dispatchEvent(new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: rect.left + rect.width / 2,
      clientY: rect.top + rect.height / 2,
    }));
  }
</script>

<ContextMenu.Root bind:open>
  <ContextMenu.Trigger
    {id}
    class="chart-pane"
    bind:ref={pane}
    disabled={busy}
    oncontextmenu={prepareMenu}
    onkeydown={handleKeydown}
  >
    {#snippet child({ props: { disabled: _disabled, style: _style, ...props } })}
      <div {...props}>
        {@render children()}
      </div>
    {/snippet}
  </ContextMenu.Trigger>
  <ContextMenu.Content
    onCloseAutoFocus={(event) => {
      event.preventDefault();
      if (!busy && returnTarget?.isConnected) returnTarget.focus({ preventScroll: true });
    }}
  >
    <ContextMenu.CheckboxItem bind:checked={showPercentage}>
      Show percentage
    </ContextMenu.CheckboxItem>
  </ContextMenu.Content>
</ContextMenu.Root>
