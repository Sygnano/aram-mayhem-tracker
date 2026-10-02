import { Component, type ErrorInfo, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";

/** How long a view that threw stays down before it is tried again. */
const RETRY_AFTER_MS = 3000;

const reported = new Set<string>();

/**
 * Sends an error to the backend's log file. The webview has no log of its own, and the overlay has
 * no console anyone will ever open, so without this a broken view is simply blank.
 *
 * Each distinct message is sent once: a view that throws on every snapshot would otherwise write
 * the same lines several times a second.
 */
export function reportError(message: string, stack?: string | null) {
  if (reported.has(message)) return;
  reported.add(message);
  invoke("report_frontend_error", { message, stack: stack ?? null }).catch(() => {});
}

/**
 * Catches a render error instead of letting React unmount the whole window.
 *
 * The view is tried again a few seconds later. What it draws comes from the snapshot, which changes
 * all the time, so an error caused by one unusual snapshot is usually gone by the next: staying down
 * for the rest of the game would turn a moment's bad data into a dead overlay.
 */
export class ErrorBoundary extends Component<{ children: ReactNode; fallback?: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  private retry: ReturnType<typeof setTimeout> | undefined;

  static getDerivedStateFromError() {
    return { failed: true };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    const message = error instanceof Error ? error.message : String(error);
    const stack = error instanceof Error ? error.stack : undefined;
    reportError(message, `${stack ?? ""}\n${info.componentStack ?? ""}`);
    clearTimeout(this.retry);
    this.retry = setTimeout(() => this.setState({ failed: false }), RETRY_AFTER_MS);
  }

  componentWillUnmount() {
    clearTimeout(this.retry);
  }

  render() {
    return this.state.failed ? (this.props.fallback ?? null) : this.props.children;
  }
}
