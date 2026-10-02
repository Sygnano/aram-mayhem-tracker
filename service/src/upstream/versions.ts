/** `versions.json`, the only mutable document upstream, polled for the current patch. */

import type { Logger } from "pino";
import type { Store, VersionEntry } from "../db/store.ts";
import type { UpstreamClient } from "./client.ts";
import { arr, int, obj, str } from "./shapes.ts";

/** Parses `versions.json`. Unlike the per-patch documents, a malformed one is an error. */
export function readVersions(value: unknown): { latest: string; versions: VersionEntry[] } {
  const o = obj(value);
  if (typeof o.latest !== "string" || !Array.isArray(o.versions)) {
    throw new Error("versions.json has no `latest` or no `versions`");
  }
  const versions = arr(o.versions).map((v) => {
    const entry = obj(v);
    if (typeof entry.version !== "string" || typeof entry.dataPath !== "string") {
      throw new Error("a versions.json entry has no `version` or no `dataPath`");
    }
    return {
      version: entry.version,
      dataPath: entry.dataPath,
      resourcePath: str(entry.resourcePath),
      dataDate: str(entry.dataDate),
      buildTimeUnixMs: int(entry.buildTimeUnixMs),
      allMatches: int(entry.allMatches),
      highMatches: int(entry.highMatches),
    };
  });
  return { latest: o.latest, versions };
}

export interface PollOptions {
  client: UpstreamClient;
  store: Store;
  aramkitBase: string;
  everyMs: number;
  log: Logger;
}

/** Reads `versions.json` once, records what it lists, prunes the rest, and returns the latest patch. */
export async function refreshVersions({
  client,
  store,
  aramkitBase,
  log,
}: Omit<PollOptions, "everyMs">): Promise<string> {
  const url = `${aramkitBase}/data/versions.json`;
  const { body } = await client.get(url);
  const doc = readVersions(JSON.parse(body.toString("utf8")));
  if (doc.versions.length === 0) {
    throw new Error("versions.json listed no patches");
  }
  store.recordVersions(doc.versions, doc.latest);

  // aramkit itself exposes only the current patch and the one before it; anything older is dead
  // weight on the volume.
  try {
    const pruned = store.pruneOldPatches(doc.versions.map((v) => v.dataPath));
    if (pruned > 0) {
      log.info({ documents: pruned }, "pruned documents from retired patches");
    }
  } catch (error) {
    log.warn({ err: error }, "pruning retired patches failed");
  }
  return doc.latest;
}

/**
 * Polls `versions.json` until stopped. Runs once immediately, so a fresh deploy knows its patch in
 * seconds rather than hours. A failure is never fatal: the store keeps serving the last known patch.
 */
export function pollVersions(options: PollOptions): { stop: () => void } {
  let timer: NodeJS.Timeout | undefined;
  let stopped = false;

  const tick = async () => {
    try {
      const latest = await refreshVersions(options);
      options.log.info({ patch: latest }, "versions.json refreshed");
    } catch (error) {
      options.log.error({ err: error }, "could not refresh versions.json");
    }
    if (!stopped) {
      timer = setTimeout(() => void tick(), options.everyMs);
    }
  };
  void tick();

  return {
    stop: () => {
      stopped = true;
      clearTimeout(timer);
    },
  };
}
