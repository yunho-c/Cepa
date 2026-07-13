<script lang="ts">
  import type { SVGAttributes } from "svelte/elements";

  let { class: className, ...restProps }: SVGAttributes<SVGSVGElement> = $props();
</script>

<svg
  class={className}
  viewBox="0 0 100 100"
  fill="none"
  aria-hidden="true"
  {...restProps}
>
  <circle
    cx="50"
    cy="50"
    r="28"
    stroke="currentColor"
    stroke-width="14"
    stroke-dasharray="137 39"
    stroke-linecap="butt"
    transform="rotate(40 50 50)"
  />
  <circle
    class="cepa-mark-accent"
    cx="50"
    cy="50"
    r="12"
    stroke="currentColor"
    stroke-width="11"
    stroke-dasharray="47 29"
    stroke-linecap="butt"
    transform="rotate(216 50 50)"
  />
  <circle cx="50" cy="50" r="4.5" fill="currentColor"/>
</svg>
