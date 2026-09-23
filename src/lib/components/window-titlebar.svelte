<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import { Minus, Square, Copy, X } from "@lucide/svelte";
  import { nativeWindow, windowPlatform } from "$lib/window-chrome";

  let { inert = false }: { inert?: boolean } = $props();
  let maximized = $state(false);
  let focused = $state(true);
  let pending = $state(false);
  let actionError = $state("");

  onMount(() => {
    if (!nativeWindow || windowPlatform === "macos") return;
    const appWindow = getCurrentWindow();
    let disposed = false;
    let revision = 0;
    const listeners: (() => void)[] = [];
    const retain = (unlisten: () => void) => {
      if (disposed) unlisten();
      else listeners.push(unlisten);
    };
    const refresh = async () => {
      const request = ++revision;
      const state = await appWindow.isMaximized();
      if (!disposed && request === revision) maximized = state;
    };
    void refresh().catch(() => {});
    void appWindow.isFocused()
      .then((value) => { if (!disposed) focused = value; })
      .catch(() => {});
    void appWindow.onResized(() => {
      void refresh().catch(() => {});
    })
      .then(retain).catch(() => {});
    void appWindow.onFocusChanged(({ payload }) => { focused = payload; })
      .then(retain).catch(() => {});
    return () => {
      disposed = true;
      listeners.forEach((unlisten) => unlisten());
    };
  });

  async function runAction(action: "minimize" | "maximize" | "close") {
    if (pending || inert) return;
    actionError = "";
    if (!nativeWindow) {
      if (action === "maximize") maximized = !maximized;
      return;
    }
    pending = true;
    try {
      const appWindow = getCurrentWindow();
      if (action === "minimize") await appWindow.minimize();
      else if (action === "close") await appWindow.close();
      else {
        await appWindow.toggleMaximize();
        maximized = await appWindow.isMaximized();
      }
    } catch {
      actionError = `Couldn’t ${action === "maximize" ? "resize" : action} the window. Try again.`;
    } finally {
      pending = false;
    }
  }
</script>

{#if windowPlatform}
  <div
    class="window-titlebar"
    data-platform={windowPlatform}
    data-focused={focused}
    {inert}
    aria-hidden={inert ? "true" : undefined}
  >
    <div class="window-drag-region" data-tauri-drag-region aria-hidden="true"></div>
    {#if windowPlatform === "macos"}
      {#if !nativeWindow}
        <div class="traffic-light-preview" aria-hidden="true">
          <i></i><i></i><i></i>
        </div>
      {/if}
    {:else}
      <div class="window-controls" role="group" aria-label="Window controls">
        <button
          type="button"
          aria-label="Minimize window"
          title="Minimize"
          aria-disabled={pending}
          onclick={() => runAction("minimize")}
        ><Minus aria-hidden="true" /></button>
        <button
          type="button"
          aria-label={maximized ? "Restore window" : "Maximize window"}
          title={maximized ? "Restore" : "Maximize"}
          aria-disabled={pending}
          onclick={() => runAction("maximize")}
        >
          {#if maximized}
            <Copy aria-hidden="true" />
          {:else}
            <Square aria-hidden="true" />
          {/if}
        </button>
        <button
          type="button"
          class="window-close"
          aria-label="Close window"
          title="Close"
          aria-disabled={pending}
          onclick={() => runAction("close")}
        ><X aria-hidden="true" /></button>
      </div>
    {/if}
    {#if actionError}
      <p class="window-action-error" role="alert">{actionError}</p>
    {/if}
  </div>
{/if}
