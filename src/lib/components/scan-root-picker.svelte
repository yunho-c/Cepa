<script lang="ts">
  import { ChevronRight, HardDrive, LoaderCircle } from "@lucide/svelte";
  import { Button } from "$lib/components/ui/button";
  import {
    scanRootUsedPercent,
    shouldShowScanRoots,
    type ScanRoot,
    type ScanRootsStatus,
  } from "$lib/scan-roots";
  import { formatBytes } from "$lib/scanner";

  interface Props {
    roots: ScanRoot[];
    status: ScanRootsStatus;
    busy: boolean;
    preparingPath: string | null;
    onSelect: (path: string) => void;
    onRetry: () => void;
  }

  let { roots, status, busy, preparingPath, onSelect, onRetry }: Props = $props();
</script>

{#if shouldShowScanRoots(status, roots.length)}
  <section class="scan-roots" aria-labelledby="scan-roots-title">
    <div class="scan-roots-heading">
      <h2 id="scan-roots-title">Storage</h2>
      {#if roots.length > 0}
        <span>{roots.length} {roots.length === 1 ? "volume" : "volumes"}</span>
      {/if}
    </div>

    {#if status === "loading"}
      <div class="scan-root-state" role="status">
        <span class="scan-root-icon"><LoaderCircle class="is-spinning" /></span>
        <div>
          <strong>Finding storage…</strong>
          <span>Checking this device</span>
        </div>
      </div>
    {:else if roots.length > 0}
      <div class="scan-root-list">
        {#each roots as root (root.path)}
          <Button
            class="scan-root"
            variant="ghost"
            disabled={busy}
            data-preparing={preparingPath === root.path}
            aria-label={`Scan ${root.name}, ${formatBytes(root.availableBytes)} available`}
            onclick={() => onSelect(root.path)}
          >
            <span class="scan-root-icon" aria-hidden="true">
              {#if preparingPath === root.path}
                <LoaderCircle class="is-spinning" />
              {:else}
                <HardDrive />
              {/if}
            </span>
            <span class="scan-root-copy">
              <strong>{root.name}</strong>
              <span>
                {root.displayPath}{root.isRemovable ? " · External" : ""}{root.isReadOnly ? " · Read only" : ""}
              </span>
              <span class="scan-root-bar" aria-hidden="true">
                <span style:width={`${scanRootUsedPercent(root)}%`}></span>
              </span>
            </span>
            <span class="scan-root-capacity">
              <strong>{formatBytes(root.availableBytes)} free</strong>
              <span>of {formatBytes(root.totalBytes)}</span>
            </span>
            <ChevronRight class="scan-root-chevron" aria-hidden="true" />
          </Button>
        {/each}
      </div>
    {:else}
      <div class="scan-root-state scan-root-unavailable" role="status">
        <span class="scan-root-icon"><HardDrive /></span>
        <div>
          <strong>Storage isn’t available.</strong>
          <span>You can still choose any folder.</span>
        </div>
        <Button variant="ghost" size="xs" onclick={onRetry}>Try again</Button>
      </div>
    {/if}
  </section>

  <div class="scan-entry-divider" aria-hidden="true"><span>or</span></div>
{/if}
