import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ErrorBoundary, reportError } from "./ErrorBoundary";
import { Companion } from "./views/Companion";
import { Overlay } from "./views/Overlay";
import "./styles.css";

// One bundle, two windows: the window label picks the view.
const label = getCurrentWindow().label;
document.documentElement.dataset.window = label;
const isOverlay = label === "overlay";
const View = isOverlay ? Overlay : Companion;

// Errors outside rendering (an event handler, a rejected promise nobody caught) reach the log too.
window.addEventListener("error", (e) => reportError(e.message, e.error instanceof Error ? e.error.stack : null));
window.addEventListener("unhandledrejection", (e) =>
  reportError(`unhandled rejection: ${String(e.reason)}`, e.reason instanceof Error ? e.reason.stack : null),
);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    {/* The overlay draws nothing while it is down: it sits over the game. The companion says so. */}
    <ErrorBoundary
      fallback={
        isOverlay ? null : (
          <div className="page error">Something went wrong drawing this window. Trying again…</div>
        )
      }
    >
      <View />
    </ErrorBoundary>
  </StrictMode>,
);
