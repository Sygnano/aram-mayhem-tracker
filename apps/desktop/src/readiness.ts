import { decimal, detail, t } from "./i18n";
import type { Snapshot } from "./types";

export type Readiness = { level: "ok" | "waiting" | "error"; text: string };

/**
 * Is the helper ready to work? The first thing in its way, in the order it needs them: the screen
 * reader, the augment names, the statistics, then the client. A closed client is waiting, not a fault.
 * The companion window shows this as its status dot; the overlay waits on less (`overlayReady`).
 */
export function readiness(s: Snapshot): Readiness {
  // A stopped part of the backend comes first: everything below it may be reading stale state.
  if (s.diagnostics.faults.length > 0) {
    return { level: "error", text: t("{detail}. Restart the app.", { detail: detail(s.diagnostics.faults[0]) }) };
  }
  if (!s.vision.available) {
    // No reason yet means the reader's thread has not reported in, which only lasts a moment.
    return s.vision.unavailableReason
      ? { level: "error", text: detail(s.vision.unavailableReason) }
      : { level: "waiting", text: t("Starting…") };
  }
  if (!s.staticData.loaded) {
    return s.staticData.error
      ? { level: "error", text: t("Game data did not load: {detail}", { detail: detail(s.staticData.error) }) }
      : { level: "waiting", text: t("Loading game data…") };
  }
  // Only the first download holds the app up. A failed update with statistics already on disk
  // keeps them, and stays green.
  if (!s.dataset.loaded) {
    if (s.dataset.downloading) return { level: "waiting", text: t("Downloading statistics… {progress}", { progress: progress(s.dataset.downloading) }) };
    return s.dataset.error
      ? { level: "error", text: t("Statistics did not download: {detail}. Trying again every minute.", { detail: detail(s.dataset.error) }) }
      : { level: "waiting", text: t("Downloading statistics…") };
  }
  if (s.client.error) return { level: "error", text: detail(s.client.error) };
  if (!s.client.connected) return { level: "waiting", text: t("Waiting for the League client") };
  return { level: "ok", text: "OK" };
}

/**
 * May the overlay draw? Only once the statistics are downloaded and the screen reader is up (D-090).
 *
 * Deliberately narrower than `readiness`: a League client that reports an error, or stops answering
 * for a moment, must not take the overlay down in the middle of a game.
 */
export function overlayReady(s: Snapshot): boolean {
  return s.dataset.loaded && s.vision.available;
}

/** "40%", or "5.2 MB" when the size is not known. */
export function progress([received, total]: [number, number | null]): string {
  return total ? `${Math.floor((100 * received) / total)}%` : `${decimal(received / 1_000_000, 1)} MB`;
}
