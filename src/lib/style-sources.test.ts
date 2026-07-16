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

  test("keeps Home focused without dimming the brand while its snapshot is released", async () => {
    const [component, stylesheet] = await Promise.all([
      Bun.file(new URL("../App.svelte", import.meta.url)).text(),
      Bun.file(new URL("../app.css", import.meta.url)).text(),
    ]);
    const homeStart = component.indexOf('class="wordmark"');
    const homeButton = component.slice(homeStart, component.indexOf("</button>", homeStart));
    const resetStart = component.indexOf("async function reset(");
    const reset = component.slice(resetStart, component.indexOf("async function", resetStart + 1));

    expect(homeStart).toBeGreaterThan(-1);
    expect(homeButton).toContain("aria-busy={isDiscardingScan}");
    expect(homeButton).toContain("aria-disabled={isBusy}");
    expect(homeButton).not.toMatch(/^\s*disabled=/m);
    expect(reset).toContain("if (isBusy) return;");
    expect(stylesheet).toContain('.wordmark[aria-disabled="true"] { cursor: wait; }');
    expect(stylesheet).not.toContain(".wordmark:disabled");
  });
});
