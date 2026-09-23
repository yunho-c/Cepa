import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";
import { installSystemAppearance } from "$lib/appearance";
import { windowPlatform } from "$lib/window-chrome";

if (windowPlatform) document.documentElement.dataset.windowChrome = windowPlatform;

const developmentParameters = import.meta.env.DEV
  ? new URLSearchParams(window.location.search)
  : null;
const appearancePreview = developmentParameters?.get("appearance");
const forcedDark =
  appearancePreview === "dark"
    ? true
    : appearancePreview === "light"
      ? false
      : undefined;
const stopAppearanceSync = installSystemAppearance(
  document.documentElement,
  window.matchMedia("(prefers-color-scheme: dark)"),
  forcedDark,
);
if (import.meta.hot) import.meta.hot.dispose(stopAppearanceSync);

const mockScenario = developmentParameters?.get("mock");
if (mockScenario) {
  const { installDevMock } = await import("$lib/dev-mock");
  installDevMock(mockScenario);
}

const app = mount(App, {
  target: document.getElementById("app")!,
});

export default app;
