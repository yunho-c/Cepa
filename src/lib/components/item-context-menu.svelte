<script lang="ts">
  import { FolderSearch } from "@lucide/svelte";
  import type { ContextMenu as ContextMenuPrimitive } from "bits-ui";
  import { onDestroy, type Snippet } from "svelte";
  import * as ContextMenu from "$lib/components/ui/context-menu";
  import * as Tooltip from "$lib/components/ui/tooltip";

  const revealLabel = /Mac/i.test(navigator.platform)
    ? "Reveal in Finder"
    : /Win/i.test(navigator.platform)
      ? "Show in File Explorer"
      : "Show in file manager";

  let {
    children,
    tooltip,
    canReveal,
    busy,
    revealing,
    onreveal,
    onmenuopenchange,
    onkeydown,
    ...buttonProps
  }: Omit<ContextMenuPrimitive.TriggerProps, "child" | "children" | "disabled"> & {
    children?: Snippet;
    tooltip?: string;
    canReveal: boolean;
    busy: boolean;
    revealing: boolean;
    onreveal: () => Promise<boolean>;
    onmenuopenchange: (open: boolean) => void;
  } = $props();

  let trigger: HTMLElement | null = $state(null);
  let open = $state(false);
  let tooltipOpen = $state(false);
  let restoreFocus = true;
  let menuGeneration = 0;

  $effect(() => {
    if (busy && open) open = false;
  });
  $effect(() => {
    if ((busy || open) && tooltipOpen) tooltipOpen = false;
  });

  $effect(() => onmenuopenchange(open));
  onDestroy(() => onmenuopenchange(false));

  function openChanged(value: boolean) {
    if (value) {
      menuGeneration += 1;
      restoreFocus = true;
    }
  }

  function handleKeydown(event: KeyboardEvent & { currentTarget: HTMLDivElement }) {
    const menuKey = event.key === "ContextMenu"
      || (event.key === "F10" && event.shiftKey);
    if (menuKey && !event.altKey && !event.ctrlKey && !event.metaKey) {
      event.preventDefault();
      if (busy || !canReveal || event.repeat || !trigger) return;
      const rect = trigger.getBoundingClientRect();
      trigger.dispatchEvent(new MouseEvent("contextmenu", {
        bubbles: true,
        cancelable: true,
        clientX: rect.left + Math.min(24, rect.width / 2),
        clientY: rect.bottom,
      }));
      return;
    }
    onkeydown?.(event);
  }

  async function reveal(event: Event) {
    // Keep the menu item mounted and focused while its request is pending.
    event.preventDefault();
    if (busy || revealing || !canReveal) return;
    const generation = menuGeneration;
    const succeeded = await onreveal();
    if (generation !== menuGeneration) return;
    // Failed requests focus the contextual result error in the parent.
    restoreFocus = succeeded;
    open = false;
  }
</script>

<ContextMenu.Root bind:open onOpenChange={openChanged}>
  <ContextMenu.Trigger
    {...buttonProps}
    bind:ref={trigger}
    disabled={busy || !canReveal}
    aria-haspopup={canReveal ? "menu" : undefined}
    aria-expanded={canReveal ? open : undefined}
    onkeydown={handleKeydown}
    oncontextmenu={(event) => {
      if (busy || !canReveal) {
        event.preventDefault();
        return;
      }
      trigger?.focus();
    }}
  >
    <!-- The primitive defaults to a div; keep the row button focusable and let
         the busy list's pointer-events rule apply to it. -->
    {#snippet child({ props: { disabled: _disabled, style: _style, ...props } })}
      {#if tooltip}
        <!-- Guard opening without changing the shared row trigger's focus behavior. -->
        <Tooltip.Root bind:open={() => tooltipOpen, (value) => tooltipOpen = value && !busy && !open}>
          <Tooltip.Trigger {...props} type="button">
            {@render children?.()}
          </Tooltip.Trigger>
          <Tooltip.Content class="row-details-tooltip" side="bottom" align="start" sideOffset={6}>
            {tooltip}
          </Tooltip.Content>
        </Tooltip.Root>
      {:else}
        <button {...props} type="button">
          {@render children?.()}
        </button>
      {/if}
    {/snippet}
  </ContextMenu.Trigger>
  {#if canReveal}
    <ContextMenu.Content
      class="item-context-menu"
      onCloseAutoFocus={(event) => {
        if (!restoreFocus || busy) event.preventDefault();
      }}
    >
      <ContextMenu.Item
        data-busy={revealing}
        onSelect={reveal}
      >
        {#snippet child({ props })}
          <div {...props} aria-disabled={busy || revealing}>
            <FolderSearch />
            {revealLabel}
          </div>
        {/snippet}
      </ContextMenu.Item>
    </ContextMenu.Content>
  {/if}
</ContextMenu.Root>
