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
});
