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
});
