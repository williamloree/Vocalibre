import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { syncLanguage } from "./i18n";
import { Overlay } from "./Overlay";
import { SettingsApp } from "./settings/App";

// One bundle, two windows: the overlay pill and the settings window.
const isOverlay = getCurrentWindow().label === "overlay";
syncLanguage();

if (isOverlay) {
  await import("./overlay.css");
} else {
  await import("./settings/app.css");
}

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(<StrictMode>{isOverlay ? <Overlay /> : <SettingsApp />}</StrictMode>);
}
