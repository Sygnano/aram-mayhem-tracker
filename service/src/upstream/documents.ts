/**
 * The upstream documents the service uses.
 *
 * aramkit documents are only ever read from the store. The crawler (`scripts/crawl.ts`) fetches them
 * ahead of time and a data version is served only once it holds them all (D-090), so a request never
 * waits on aramkit, and aramkit only ever sees the crawler's slow and steady pace.
 *
 * CommunityDragon documents are still fetched on a miss. Two rules shape that: everything is pinned,
 * so a document we hold is never revalidated; and a miss is fetched once however many requests want
 * it. The fetch belongs to no request, so it runs to the end and fills the store even if every
 * request waiting on it has gone away.
 */

import { aramkitKey, cdragonKey, type DocKey, type Store } from "../db/store.ts";
import { UnavailableError, UpstreamStatusError } from "../errors.ts";
import type { UpstreamClient } from "./client.ts";

/** Documents we fetch. Each is the `kind` column of its rows. */
export const CHAMPION_DETAILS = "champion-details";
export const AUGMENT_RANKINGS = "augment-rankings";
export const CHAMPION_RANKINGS = "champion-rankings";
export const CHERRY_AUGMENTS = "cherry-augments";
export const AUGMENT_LISTS = "augment-lists";
export const ANVIL_MAP = "map12-bin";

/** Where an aramkit document lives, under aramkit's base URL. */
export function aramkitPath(dataPath: string, kind: string, key: string): string {
  return kind === CHAMPION_DETAILS
    ? `${dataPath}/stats/all/${kind}/${key}.json`
    : `${dataPath}/stats/all/${kind}.json`;
}

export interface DocumentsOptions {
  store: Store;
  client: UpstreamClient;
  cdragonBase: string;
}

export class Documents {
  readonly #store: Store;
  readonly #client: UpstreamClient;
  readonly cdragonBase: string;
  /** Fetches in progress, by key. Exposed read-only so tests can see the map empties. */
  readonly inFlight = new Map<string, Promise<Buffer>>();

  constructor({ store, client, cdragonBase }: DocumentsOptions) {
    this.#store = store;
    this.#client = client;
    this.cdragonBase = cdragonBase;
  }

  /**
   * A crawled aramkit document. One the crawler found missing upstream answers as aramkit did, 404;
   * one it has not reached yet is unavailable, which only happens for a version not served yet.
   */
  #crawled(key: DocKey): Promise<Buffer> {
    const held = this.#store.getDoc(key);
    if (held !== null) {
      return Promise.resolve(held);
    }
    const path = aramkitPath(key.dataPath, key.kind, key.key);
    return Promise.reject(
      this.#store.isAbsent(key)
        ? new UpstreamStatusError(404, path, "(recorded by the crawler)")
        : new UnavailableError(`${path} has not been crawled`),
    );
  }

  /**
   * The document for `key`, fetched only if the store does not already have it. A hit is one SQLite
   * read; on a miss, every caller waits on the one fetch.
   */
  ensure(key: DocKey, url: string): Promise<Buffer> {
    const held = this.#store.getDoc(key);
    if (held !== null) {
      return Promise.resolve(held);
    }
    const id = JSON.stringify([key.dataPath, key.source, key.kind, key.key]);
    let pending = this.inFlight.get(id);
    if (pending === undefined) {
      pending = this.#fetchAndStore(key, url).finally(() => this.inFlight.delete(id));
      this.inFlight.set(id, pending);
    }
    return pending;
  }

  async #fetchAndStore(key: DocKey, url: string): Promise<Buffer> {
    // A fetch that finished between the caller's store read and this one has already filled it.
    const held = this.#store.getDoc(key);
    if (held !== null) {
      return held;
    }
    const failure = this.#store.failureBackoff(key);
    if (failure !== null) {
      // Rebuilt with its status, so a repeat request answers the way the first one did.
      const note = "(recent failure, not retrying yet)";
      throw failure.status === null
        ? new Error(`${failure.message} ${note}`)
        : new UpstreamStatusError(failure.status, url, note);
    }

    try {
      const { etag, body } = await this.#client.get(url);
      this.#store.putDoc(key, body, etag);
      this.#store.clearFailure(key);
      return body;
    } catch (error) {
      const status = error instanceof UpstreamStatusError ? error.status : null;
      this.#store.recordFailure(
        key,
        status,
        error instanceof Error ? error.message : String(error),
      );
      throw error;
    }
  }

  // -- the documents we actually want ---------------------------------------------------------

  championDetails(dataPath: string, championId: number): Promise<Buffer> {
    return this.#crawled(aramkitKey(dataPath, CHAMPION_DETAILS, String(championId)));
  }

  augmentRankings(dataPath: string): Promise<Buffer> {
    return this.#crawled(aramkitKey(dataPath, AUGMENT_RANKINGS));
  }

  /**
   * Every champion's rank, tier and rates in one document, what champ select is built on. Singleton
   * per build, so one fetch per patch serves every champion on the screen.
   */
  championRankings(dataPath: string): Promise<Buffer> {
    return this.#crawled(aramkitKey(dataPath, CHAMPION_RANKINGS));
  }

  /**
   * CommunityDragon augment metadata: rarity, name id and icon, keyed by Riot augment id.
   * Pinned to the patch so a cached copy is not silently replaced mid-patch.
   */
  cherryAugments(patch: string): Promise<Buffer> {
    return this.ensure(
      cdragonKey(CHERRY_AUGMENTS, patch),
      `${this.cdragonBase}/${patch}/plugins/rcp-be-lol-game-data/global/default/v1/cherry-augments.json`,
    );
  }

  /** The Mayhem pools (`KIWI`, `KIWI_JADE`), which decide pool membership for `/v1/pool`. */
  augmentLists(patch: string): Promise<Buffer> {
    return this.ensure(
      cdragonKey(AUGMENT_LISTS, patch),
      `${this.cdragonBase}/${patch}/plugins/rcp-be-lol-game-data/global/default/v1/augment-lists.json`,
    );
  }

  /**
   * The ARAM map bin, which holds Mayhem's stat anvil shards. About 6.8 MB raw and well under
   * 1 MB gzipped, so it is kept like any other upstream document.
   */
  anvilMap(patch: string): Promise<Buffer> {
    return this.ensure(
      cdragonKey(ANVIL_MAP, patch),
      `${this.cdragonBase}/${patch}/game/data/maps/shipping/map12/map12.bin.json`,
    );
  }

  /**
   * One locale's game string table, fetched and **not** stored. It is ~31 MB and only about 40 of its
   * 200k strings are wanted; the derived anvil catalogue keeps those. A rebuild refetches it, which
   * happens once per patch per locale.
   *
   * The inner folder is `en_us` for every locale: `game/fr_fr/data/menu/en_us/` is the French table.
   * Checked on 16.19.
   */
  async stringTable(patch: string, locale: string): Promise<Buffer> {
    const url = `${this.cdragonBase}/${patch}/game/${locale}/data/menu/en_us/lol.stringtable.json`;
    return (await this.#client.get(url)).body;
  }
}
