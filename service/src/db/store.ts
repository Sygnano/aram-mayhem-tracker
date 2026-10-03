/**
 * Every query the service runs, over the one connection `openDatabase` returns.
 *
 * Two cache layers. `upstream_docs` holds raw bodies exactly as fetched and is only ever inserted
 * into, because everything under an aramkit `dataPath` is immutable. `derived` holds parsed
 * payloads and can be dropped and rebuilt from the raw layer without refetching. Both are gzipped.
 */

import { gunzipSync, gzipSync } from "node:zlib";
import { type Connection, nowUnix } from "./database.ts";

/**
 * Bumped when a parser changes shape, so a deploy starts writing fresh `derived` rows and ignores
 * the ones an older build left behind. 2 is the TypeScript service; the Rust one wrote 1, and the
 * two never read each other's rows, which keeps a rollback clean.
 */
export const SCHEMA_VER = 2;

/** Anvil ranking saves kept. Far more than hand editing needs to undo a mistake, and it bounds the table. */
export const HISTORY_KEPT = 200;

/**
 * Identifies one upstream document. `dataPath` is empty for CommunityDragon, which is pinned by patch
 * rather than by an aramkit build.
 */
export interface DocKey {
  dataPath: string;
  source: "aramkit" | "cdragon";
  kind: string;
  key: string;
}

export function aramkitKey(dataPath: string, kind: string, key = ""): DocKey {
  return { dataPath, source: "aramkit", kind, key };
}

export function cdragonKey(kind: string, key: string): DocKey {
  return { dataPath: "", source: "cdragon", kind, key };
}

/** A recorded upstream failure that is still being honoured. */
export interface HeldFailure {
  status: number | null;
  message: string;
}

/** One entry of `versions.json`. */
export interface VersionEntry {
  version: string;
  dataPath: string;
  resourcePath: string;
  dataDate: string;
  buildTimeUnixMs: number;
  allMatches: number;
  highMatches: number;
}

/**
 * The data version the service answers with: the newest one whose crawl has finished. `null` until a
 * crawl first completes.
 */
export interface LatestVersion {
  version: string;
  dataPath: string;
  dataDate: string;
  allMatches: number;
  firstSeenAt: number;
}

/** The version the crawler works on, and whether it has finished. */
export interface CrawlTarget {
  version: string;
  dataPath: string;
  dataDate: string;
  /** Unix seconds when the last document arrived; `null` while the crawl is still going. */
  crawledAt: number | null;
}

function docKeyArgs(key: DocKey): [string, string, string, string] {
  return [key.dataPath, key.source, key.kind, key.key];
}

export class Store {
  readonly db: Connection;

  constructor(db: Connection) {
    this.db = db;
  }

  /** A cheap statement that fails if the file or the volume has gone away. */
  ping(): void {
    this.db.prepare("SELECT 1").get();
  }

  /**
   * The version every data endpoint answers for: the newest one held in full. A newer version that
   * is still being crawled is not served, so no request ever needs a document that is not here yet.
   */
  latestVersion(): LatestVersion | null {
    const row = this.db
      .prepare<[], LatestVersion>(
        `SELECT version, data_path AS dataPath, data_date AS dataDate,
                all_matches AS allMatches, first_seen_at AS firstSeenAt
           FROM versions WHERE crawled_at IS NOT NULL
          ORDER BY build_time_ms DESC LIMIT 1`,
      )
      .get();
    return row ?? null;
  }

  /** What `versions.json` last called the latest: the version the crawler works on. */
  crawlTarget(): CrawlTarget | null {
    const row = this.db
      .prepare<[], CrawlTarget>(
        `SELECT version, data_path AS dataPath, data_date AS dataDate, crawled_at AS crawledAt
           FROM versions WHERE is_latest = 1
          ORDER BY build_time_ms DESC LIMIT 1`,
      )
      .get();
    return row ?? null;
  }

  /** Marks `dataPath` as held in full, which makes it servable. Returns false if it was already. */
  markCrawled(dataPath: string): boolean {
    return (
      this.db
        .prepare("UPDATE versions SET crawled_at = ? WHERE data_path = ? AND crawled_at IS NULL")
        .run(nowUnix(), dataPath).changes > 0
    );
  }

  /**
   * How many champion documents we hold for `dataPath`. Zero is the normal state on a fresh deploy
   * and on the day a new patch lands.
   */
  championsCached(dataPath: string): number {
    const row = this.db
      .prepare<[string], { n: number }>(
        `SELECT COUNT(*) AS n FROM upstream_docs
          WHERE data_path = ? AND source = 'aramkit' AND kind = 'champion-details'`,
      )
      .get(dataPath);
    return row?.n ?? 0;
  }

  // -- raw upstream documents -------------------------------------------------------------------

  /** The raw body for `key`, decompressed, or `null` if we have never fetched it. */
  getDoc(key: DocKey): Buffer | null {
    const row = this.db
      .prepare<[string, string, string, string], { body: Buffer }>(
        `SELECT body FROM upstream_docs
          WHERE data_path = ? AND source = ? AND kind = ? AND key = ?`,
      )
      .get(key.dataPath, key.source, key.kind, key.key);
    return row ? gunzipSync(row.body) : null;
  }

  putDoc(key: DocKey, body: Buffer, etag: string | null): void {
    this.db
      .prepare(
        `INSERT OR REPLACE INTO upstream_docs
             (data_path, source, kind, key, body, etag, fetched_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)`,
      )
      .run(key.dataPath, key.source, key.kind, key.key, gzipSync(body), etag, nowUnix());
  }

  /** True if `key` is held, or recorded as absent upstream. Either way the crawl has it covered. */
  hasDocOrAbsent(key: DocKey): boolean {
    const row = this.db
      .prepare<[string, string, string, string, string, string, string, string], { n: number }>(
        `SELECT (SELECT COUNT(*) FROM upstream_docs
                  WHERE data_path = ? AND source = ? AND kind = ? AND key = ?)
              + (SELECT COUNT(*) FROM absent_docs
                  WHERE data_path = ? AND source = ? AND kind = ? AND key = ?) AS n`,
      )
      .get(...docKeyArgs(key), ...docKeyArgs(key));
    return (row?.n ?? 0) > 0;
  }

  /** True if upstream answered 404 for `key`. */
  isAbsent(key: DocKey): boolean {
    return (
      this.db
        .prepare<[string, string, string, string], { n: number }>(
          `SELECT COUNT(*) AS n FROM absent_docs
            WHERE data_path = ? AND source = ? AND kind = ? AND key = ?`,
        )
        .get(...docKeyArgs(key))?.n === 1
    );
  }

  /** Records that upstream has no such document. Immutable paths stay that way. */
  putAbsent(key: DocKey): void {
    this.db
      .prepare(
        `INSERT OR REPLACE INTO absent_docs (data_path, source, kind, key, checked_at)
         VALUES (?, ?, ?, ?, ?)`,
      )
      .run(...docKeyArgs(key), nowUnix());
  }

  // -- derived payloads -------------------------------------------------------------------------

  /** A derived payload for the current `SCHEMA_VER`, decompressed, or `null`. */
  getDerived(dataPath: string, kind: string, key: string): Buffer | null {
    const row = this.db
      .prepare<[string, string, string, number], { body: Buffer }>(
        `SELECT body FROM derived
          WHERE data_path = ? AND kind = ? AND key = ? AND schema_ver = ?`,
      )
      .get(dataPath, kind, key, SCHEMA_VER);
    return row ? gunzipSync(row.body) : null;
  }

  putDerived(dataPath: string, kind: string, key: string, body: Buffer): void {
    this.db
      .prepare(
        `INSERT OR REPLACE INTO derived (data_path, kind, key, schema_ver, body, built_at)
         VALUES (?, ?, ?, ?, ?, ?)`,
      )
      .run(dataPath, kind, key, SCHEMA_VER, gzipSync(body), nowUnix());
  }

  // -- upstream failures ------------------------------------------------------------------------

  /** Records an upstream failure, so a champion that 404s is not refetched on every request. */
  recordFailure(key: DocKey, status: number | null, message: string): void {
    this.db
      .prepare(
        `INSERT INTO fetch_failures
             (data_path, source, kind, key, status, message, failed_at, attempts)
         VALUES (?, ?, ?, ?, ?, ?, ?, 1)
         ON CONFLICT (data_path, source, kind, key) DO UPDATE SET
             status = excluded.status, message = excluded.message,
             failed_at = excluded.failed_at, attempts = fetch_failures.attempts + 1`,
      )
      .run(key.dataPath, key.source, key.kind, key.key, status, message, nowUnix());
  }

  /**
   * The recorded failure for `key` while it is still being honoured.
   *
   * A 404 is a statement of fact about an immutable path, so it is honoured for an hour rather than
   * retried per request. Anything else backs off briefly and then tries again. The status comes back
   * with the message so a repeat request answers 404 exactly as the first one did.
   */
  failureBackoff(key: DocKey): HeldFailure | null {
    const row = this.db
      .prepare<
        [string, string, string, string],
        { status: number | null; message: string | null; failedAt: number }
      >(
        `SELECT status, message, failed_at AS failedAt FROM fetch_failures
          WHERE data_path = ? AND source = ? AND kind = ? AND key = ?`,
      )
      .get(key.dataPath, key.source, key.kind, key.key);
    if (!row) {
      return null;
    }
    const hold = row.status === 404 ? 3600 : 30;
    return nowUnix() - row.failedAt < hold
      ? { status: row.status, message: row.message ?? "" }
      : null;
  }

  clearFailure(key: DocKey): void {
    this.db
      .prepare(
        `DELETE FROM fetch_failures
          WHERE data_path = ? AND source = ? AND kind = ? AND key = ?`,
      )
      .run(key.dataPath, key.source, key.kind, key.key);
  }

  // -- versions ---------------------------------------------------------------------------------

  /**
   * Records the patches `versions.json` reported, marking `latest` and leaving older rows in place so
   * a request mid-rollover still resolves.
   */
  recordVersions(versions: readonly VersionEntry[], latest: string): void {
    const clear = this.db.prepare("UPDATE versions SET is_latest = 0");
    const insert = this.db.prepare(
      `INSERT INTO versions
           (version, data_path, resource_path, data_date, build_time_ms,
            all_matches, high_matches, is_latest, first_seen_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
       ON CONFLICT (data_path) DO UPDATE SET is_latest = excluded.is_latest`,
    );
    this.db.transaction(() => {
      clear.run();
      for (const v of versions) {
        insert.run(
          v.version,
          v.dataPath,
          v.resourcePath,
          v.dataDate,
          v.buildTimeUnixMs,
          v.allMatches,
          v.highMatches,
          v.version === latest ? 1 : 0,
          nowUnix(),
        );
      }
    })();
  }

  /** Deletes everything belonging to patches we no longer serve, keeping `keep` data paths. */
  pruneOldPatches(keep: readonly string[]): number {
    if (keep.length === 0) {
      return 0;
    }
    const placeholders = keep.map(() => "?").join(",");
    return this.db.transaction(() => {
      // `data_path = ''` is CommunityDragon, which is not tied to an aramkit build.
      const docs = this.db
        .prepare(
          `DELETE FROM upstream_docs WHERE data_path <> '' AND data_path NOT IN (${placeholders})`,
        )
        .run(...keep).changes;
      this.db
        .prepare(`DELETE FROM derived WHERE data_path <> '' AND data_path NOT IN (${placeholders})`)
        .run(...keep);
      this.db
        .prepare(
          `DELETE FROM fetch_failures WHERE data_path <> '' AND data_path NOT IN (${placeholders})`,
        )
        .run(...keep);
      this.db
        .prepare(`DELETE FROM absent_docs WHERE data_path NOT IN (${placeholders})`)
        .run(...keep);
      this.db.prepare(`DELETE FROM versions WHERE data_path NOT IN (${placeholders})`).run(...keep);
      return docs;
    })();
  }

  // -- anvil rankings ---------------------------------------------------------------------------

  /** The current rankings document as saved, and when; `null` before the first save. */
  anvilRankings(): { body: string; savedAt: number } | null {
    const row = this.db
      .prepare<[], { body: string; savedAt: number }>(
        "SELECT body, saved_at AS savedAt FROM anvil_rankings ORDER BY id DESC LIMIT 1",
      )
      .get();
    return row ?? null;
  }

  /**
   * Appends a save, which makes it the current document, and returns how many saves are kept.
   * Earlier saves stay in the table, the newest `HISTORY_KEPT` of them.
   */
  saveAnvilRankings(body: string, savedAt: number): number {
    return this.db.transaction(() => {
      this.db
        .prepare("INSERT INTO anvil_rankings (body, saved_at) VALUES (?, ?)")
        .run(body, savedAt);
      this.db
        .prepare(
          `DELETE FROM anvil_rankings
            WHERE id <= (SELECT MAX(id) FROM anvil_rankings) - ?`,
        )
        .run(HISTORY_KEPT);
      const row = this.db
        .prepare<[], { n: number }>("SELECT COUNT(*) AS n FROM anvil_rankings")
        .get();
      return row?.n ?? 0;
    })();
  }
}
