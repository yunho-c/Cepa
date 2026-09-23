import { isTauri } from "@tauri-apps/api/core";

export type WindowPlatform = "macos" | "windows" | "linux";

// Evaluated before development scan mocks install an IPC bridge. Browser
// previews must never accidentally invoke real window-management commands.
export const nativeWindow = isTauri();
const preview = import.meta.env.DEV
  ? new URLSearchParams(window.location.search).get("titlebar")
  : null;
export const windowPlatform: WindowPlatform | null =
  nativeWindow
    ? /Mac/i.test(navigator.platform)
      ? "macos"
      : /Win/i.test(navigator.platform)
        ? "windows"
        : "linux"
    : preview === "macos" || preview === "windows" || preview === "linux"
      ? preview
      : null;
