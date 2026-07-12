import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";

const mockScenario = import.meta.env.DEV
  ? new URLSearchParams(window.location.search).get("mock")
  : null;
if (mockScenario) {
  const { installDevMock } = await import("$lib/dev-mock");
  installDevMock(mockScenario);
}

const app = mount(App, {
  target: document.getElementById("app")!,
});

export default app;
