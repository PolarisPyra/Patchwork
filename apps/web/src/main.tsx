import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/inter";
import "@fontsource-variable/jetbrains-mono";
import "./theme.css";
import App from "~/app/App";
import { installRendererLog } from "~/lib/rendererLog";

installRendererLog();

// Marks the dev session: theme.css frames it in hazard stripes, and agents can read the flag.
if (import.meta.env.DEV) document.documentElement.dataset.devSession = "true";

// The webview's own menu (Reload, Back, Inspect) never opens. Menus the app draws prevent the
// default first; text fields and selected text keep the native Cut, Copy and Paste.
window.addEventListener("contextmenu", (event) => {
  if (event.defaultPrevented) return;
  const target = event.target instanceof Element ? event.target : null;
  if (target?.closest("input, textarea, [contenteditable]:not([contenteditable='false'])")) return;
  if (window.getSelection()?.isCollapsed === false) return;
  event.preventDefault();
});

// The desktop window has no reload shortcut, and the composer editor only rebuilds on a
// fresh load, so Ctrl+R reloads the webview during development.
if (import.meta.env.DEV) {
  window.addEventListener("keydown", (event) => {
    if (event.ctrlKey && !event.shiftKey && event.key.toLowerCase() === "r") {
      event.preventDefault();
      window.location.reload();
    }
  });
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
