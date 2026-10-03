/** `versions.json`, the only mutable document upstream, which names the current data version. */

import type { Logger } from "pino";
import type { Store, VersionEntry } from "../db/store.ts";
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

/**
 * Records what a `versions.json` body lists, marks its latest, and prunes the rest. Returns the
 * latest patch. The crawler fetches the document (D-090); the service only reads it.
 *
 * Pruning keeps the version currently served even if `versions.json` no longer lists it, so a
 * rollover never leaves the service with nothing to answer from while the new one is crawled.
 */
export function applyVersions(store: Store, body: Buffer, log: Logger): VersionEntry {
  const doc = readVersions(JSON.parse(body.toString("utf8")));
  const latest = doc.versions.find((v) => v.version === doc.latest);
  if (latest === undefined) {
    throw new Error(`versions.json lists no entry for its latest, ${doc.latest}`);
  }
  store.recordVersions(doc.versions, doc.latest);

  // aramkit itself exposes only the current patch and the one before it; anything older is dead
  // weight on the volume.
  try {
    const keep = doc.versions.map((v) => v.dataPath);
    const served = store.latestVersion();
    if (served !== null && !keep.includes(served.dataPath)) {
      keep.push(served.dataPath);
    }
    const pruned = store.pruneOldPatches(keep);
    if (pruned > 0) {
      log.info({ documents: pruned }, "pruned documents from retired patches");
    }
  } catch (error) {
    log.warn({ err: error }, "pruning retired patches failed");
  }
  return latest;
}
