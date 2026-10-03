/**
 * The service's side of the crawl (D-090): what the crawler still has to fetch, taking in what it
 * sends, and switching a data version live once every document is held.
 *
 * The crawler (`scripts/crawl.ts`) runs as its own Railway service on a cron, and the database is a
 * SQLite file on this service's volume, so the crawler cannot write to it. It fetches from aramkit
 * and sends each document here, and this stays the only process that writes the database.
 *
 * A version is made of `augment-rankings.json`, `champion-rankings.json`, and one
 * `champion-details/<id>.json` per champion the rankings list. The champion list is only known once
 * the rankings are in, so what is missing is worked out afresh after every document.
 */

import type { Logger } from "pino";
import type { Data } from "./data.ts";
import { aramkitKey, type Store } from "./db/store.ts";
import { BadRequestError, ConflictError } from "./errors.ts";
import {
  AUGMENT_RANKINGS,
  aramkitPath,
  CHAMPION_DETAILS,
  CHAMPION_RANKINGS,
} from "./upstream/documents.ts";
import { readAugmentRows, readChampionDetails, readChampionRankings } from "./upstream/shapes.ts";
import { applyVersions } from "./upstream/versions.ts";

/** One document the crawler has yet to fetch. `path` is relative to aramkit's base URL. */
export interface MissingDoc {
  kind: string;
  key: string;
  path: string;
}

/** Where the crawl of the latest data version stands. What every crawl route answers with. */
export interface CrawlStatus {
  /** The version `versions.json` last called the latest; `null` before it was first sent. */
  dataPath: string | null;
  version: string | null;
  /** Every document is held, and the version is being served. */
  complete: boolean;
  /** Documents held or known to be absent upstream. */
  done: number;
  /** What a full version is, once the champion list is known; `null` before. */
  expected: number | null;
  /** Why the crawl cannot complete however long it runs, if it cannot. */
  blocked: string | null;
  /** What is left, in the order to fetch it: the two rankings first, then champions by id. */
  missing: MissingDoc[];
}

export interface CrawlOptions {
  store: Store;
  data: Data;
  log: Logger;
  /** Called once when a version becomes servable, so its dataset can be built before it is asked for. */
  onComplete?: (dataPath: string) => void;
}

const SINGLETONS = [AUGMENT_RANKINGS, CHAMPION_RANKINGS];

export class Crawl {
  readonly #store: Store;
  readonly #data: Data;
  readonly #log: Logger;
  readonly #onComplete: (dataPath: string) => void;

  constructor({ store, data, log, onComplete = () => {} }: CrawlOptions) {
    this.#store = store;
    this.#data = data;
    this.#log = log;
    this.#onComplete = onComplete;
  }

  /** Takes in a `versions.json` body, which picks the version to crawl. */
  async versions(body: Buffer): Promise<CrawlStatus> {
    let latest: ReturnType<typeof applyVersions>;
    try {
      latest = applyVersions(this.#store, body, this.#log);
    } catch (error) {
      throw new BadRequestError(`versions.json not taken: ${String(error)}`);
    }
    this.#log.info({ dataPath: latest.dataPath }, "versions.json received from the crawler");
    return this.status();
  }

  /**
   * Stores one document for the version being crawled, or records it as absent upstream when
   * `body` is `null`. A document that does not parse is refused, so the store never holds one that
   * would fail every request built on it.
   */
  async ingest(
    dataPath: string,
    kind: string,
    key: string,
    body: Buffer | null,
  ): Promise<CrawlStatus> {
    const target = this.#store.crawlTarget();
    if (target === null || target.dataPath !== dataPath) {
      throw new ConflictError(
        `${dataPath} is not the version being crawled (${target?.dataPath ?? "none yet"}); send versions.json first`,
      );
    }
    const docKey = aramkitKey(dataPath, kind, checkedKey(kind, key));
    if (body === null) {
      this.#store.putAbsent(docKey);
      this.#log.warn({ dataPath, kind, key }, "aramkit has no such document");
    } else {
      validate(kind, key, body);
      this.#store.putDoc(docKey, body, null);
    }
    return this.status();
  }

  /** What is left to fetch. Switches the version live the first time nothing is. */
  async status(): Promise<CrawlStatus> {
    const target = this.#store.crawlTarget();
    if (target === null) {
      return {
        dataPath: null,
        version: null,
        complete: false,
        done: 0,
        expected: null,
        blocked: null,
        missing: [],
      };
    }
    const { dataPath } = target;
    const covered = (kind: string, key = "") =>
      this.#store.hasDocOrAbsent(aramkitKey(dataPath, kind, key));
    const missingDoc = (kind: string, key = ""): MissingDoc => ({
      kind,
      key,
      path: aramkitPath(dataPath, kind, key),
    });

    const missing: MissingDoc[] = SINGLETONS.filter((kind) => !covered(kind)).map((kind) =>
      missingDoc(kind),
    );
    const absentSingletons = SINGLETONS.filter((kind) =>
      this.#store.isAbsent(aramkitKey(dataPath, kind)),
    );
    const blocked =
      absentSingletons.length > 0
        ? `aramkit has no ${absentSingletons.join(" or ")} for ${dataPath}; it cannot be served`
        : null;

    let champions: number[] | null = null;
    if (this.#store.getDoc(aramkitKey(dataPath, CHAMPION_RANKINGS)) !== null) {
      const table = await this.#data.championTable(dataPath);
      champions = Object.keys(table.champions)
        .map(Number)
        .sort((a, b) => a - b);
      for (const id of champions) {
        if (!covered(CHAMPION_DETAILS, String(id))) {
          missing.push(missingDoc(CHAMPION_DETAILS, String(id)));
        }
      }
    }

    const expected = champions === null ? null : SINGLETONS.length + champions.length;
    const done = (expected ?? SINGLETONS.length) - missing.length;
    const complete = champions !== null && missing.length === 0 && blocked === null;
    if (complete && target.crawledAt === null && this.#store.markCrawled(dataPath)) {
      this.#log.info({ dataPath, documents: expected }, "crawl complete, now serving this version");
      this.#onComplete(dataPath);
    }
    return { dataPath, version: target.version, complete, done, expected, blocked, missing };
  }
}

/** A champion's key is its id; the two rankings have none. */
function checkedKey(kind: string, key: string): string {
  if (kind === CHAMPION_DETAILS) {
    if (!/^[1-9]\d{0,5}$/.test(key)) {
      throw new BadRequestError(`a champion id is a positive integer, got ${JSON.stringify(key)}`);
    }
    return key;
  }
  if (SINGLETONS.includes(kind)) {
    if (key !== "") {
      throw new BadRequestError(`${kind} takes no key`);
    }
    return key;
  }
  throw new BadRequestError(`unknown document kind ${JSON.stringify(kind)}`);
}

/**
 * Parses `body` the way the data layer will, so a document that would break it is refused now. The
 * readers are lenient by design, so the checks that matter are made here: an object, a rankings
 * table with champions in it, and a champion document about the champion it was sent as.
 */
function validate(kind: string, key: string, body: Buffer): void {
  let value: unknown;
  try {
    value = JSON.parse(body.toString("utf8"));
  } catch (error) {
    throw new BadRequestError(`${kind} is not JSON: ${String(error)}`);
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new BadRequestError(`${kind} is not a JSON object`);
  }
  if (kind === AUGMENT_RANKINGS) {
    readAugmentRows(value);
  } else if (kind === CHAMPION_RANKINGS) {
    if (readChampionRankings(value).length === 0) {
      throw new BadRequestError("champion-rankings lists no champion");
    }
  } else {
    const id = readChampionDetails(value).champion.id;
    if (String(id) !== key) {
      throw new BadRequestError(`champion-details/${key} is about champion ${id}`);
    }
  }
}
