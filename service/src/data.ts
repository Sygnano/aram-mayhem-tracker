/**
 * Ties the layers together: the derived cache first, then the raw cache, then upstream.
 *
 * Each method returns a parsed structure and leaves a derived payload behind, so a document is
 * parsed once per champion per patch rather than once per request.
 */

import type { Logger } from "pino";
import type { Store } from "./db/store.ts";
import { parseAnvilDocuments } from "./domain/anvil-worker.ts";
import { type AnvilCatalogue, catalogue as anvilCatalogue } from "./domain/anvils.ts";
import {
  type AugmentCatalogue,
  type AugmentEntry,
  type ChampionAugments,
  type ChampionTable,
  catalogue,
  championAugments,
  championRankingsTable,
  globalRankings,
} from "./domain/augments.ts";
import { type ArchetypeBuild, archetypes } from "./domain/builds.ts";
import { type DamageProfile, damageProfile } from "./domain/damage.ts";
import type { Documents } from "./upstream/documents.ts";
import {
  readAugmentLists,
  readAugmentRows,
  readChampionDetails,
  readChampionRankings,
  readCherryAugments,
} from "./upstream/shapes.ts";

/** `kind` values in the `derived` table. */
const CHAMPION_AUGMENTS = "champion-augments";
const CHAMPION_BUILDS = "champion-builds";
const GLOBAL_RANKINGS = "augment-rankings";
const CATALOGUE = "augment-catalogue";
const CHAMPION_TABLE = "champion-table";
const CHAMPION_DAMAGE = "champion-damage";
const ANVIL_CATALOGUE = "anvil-catalogue";

/** The anvil pool sizes on 16.19: silver, gold, prismatic. */
const EXPECTED_ANVIL_POOLS = [13, 13, 8];

export interface DataOptions {
  store: Store;
  documents: Documents;
  log: Logger;
}

export class Data {
  readonly #store: Store;
  readonly #documents: Documents;
  readonly #log: Logger;
  /**
   * Anvil catalogue builds in progress, by `patch/locale`. A build fetches a ~31 MB string table, and
   * without this every request arriving before the first build finished would fetch its own copy.
   */
  readonly #anvilBuilds = new Map<string, Promise<AnvilCatalogue>>();

  constructor({ store, documents, log }: DataOptions) {
    this.#store = store;
    this.#documents = documents;
    this.#log = log;
  }

  /** One champion's augment table for `dataPath`. */
  championAugments(dataPath: string, championId: number): Promise<ChampionAugments> {
    return this.#derived(dataPath, CHAMPION_AUGMENTS, String(championId), async () =>
      championAugments(await this.#championDetails(dataPath, championId)),
    );
  }

  championBuilds(dataPath: string, championId: number): Promise<ArchetypeBuild[]> {
    return this.#derived(dataPath, CHAMPION_BUILDS, String(championId), async () =>
      archetypes(await this.#championDetails(dataPath, championId)),
    );
  }

  /**
   * How one champion's damage splits between physical, magic and true. Read from the same
   * `champion-details` document as the augments and the builds, so for a champion somebody has
   * already locked in it costs no upstream fetch at all.
   */
  championDamage(dataPath: string, championId: number): Promise<DamageProfile | null> {
    return this.#derived(dataPath, CHAMPION_DAMAGE, String(championId), async () =>
      damageProfile(await this.#championDetails(dataPath, championId)),
    );
  }

  /** The champion-agnostic augment table, used when a champion has no usable row. */
  globalRankings(dataPath: string): Promise<Record<number, AugmentEntry>> {
    return this.#derived(dataPath, GLOBAL_RANKINGS, "", async () => {
      const raw = await this.#documents.augmentRankings(dataPath);
      return globalRankings(readAugmentRows(parseJson(raw, "augment-rankings.json")));
    });
  }

  /**
   * Every champion's rank, tier and rates for `dataPath`. A singleton, so one upstream fetch and one
   * parse per patch however many champions are asked about.
   */
  championTable(dataPath: string): Promise<ChampionTable> {
    return this.#derived(dataPath, CHAMPION_TABLE, "", async () => {
      const raw = await this.#documents.championRankings(dataPath);
      return championRankingsTable(readChampionRankings(parseJson(raw, "champion-rankings.json")));
    });
  }

  /**
   * Rarity, names, icons and Mayhem pool membership, from CommunityDragon. Keyed by patch
   * rather than by aramkit build, since CommunityDragon does not share aramkit's paths.
   */
  catalogue(patch: string): Promise<AugmentCatalogue> {
    return this.#derived("", CATALOGUE, patch, async () => {
      const augments = readCherryAugments(
        parseJson(await this.#documents.cherryAugments(patch), "cherry-augments.json"),
      );
      const lists = readAugmentLists(
        parseJson(await this.#documents.augmentLists(patch), "augment-lists.json"),
      );
      const built = catalogue(patch, this.#documents.cdragonBase, augments, lists);
      if (built.unmatched.length > 0) {
        // Research found zero unresolved entries on 16.19, so any at all is worth seeing.
        this.#log.warn(
          { count: built.unmatched.length, patch },
          "pool entries did not resolve to an augment",
        );
      }
      return built;
    });
  }

  /**
   * The stat anvil shards for `patch`, named in `locale`. Keyed by patch and locale, so each
   * locale costs one string-table fetch per patch.
   */
  anvilCatalogue(patch: string, locale: string): Promise<AnvilCatalogue> {
    const key = `${patch}/${locale}`;
    // The cache check is inside the shared promise too: the second caller in a burst waits for the
    // first one's result instead of starting its own fetch.
    let pending = this.#anvilBuilds.get(key);
    if (pending === undefined) {
      pending = this.#derived("", ANVIL_CATALOGUE, key, () =>
        this.#buildAnvilCatalogue(patch, locale),
      ).finally(() => this.#anvilBuilds.delete(key));
      this.#anvilBuilds.set(key, pending);
    }
    return pending;
  }

  async #buildAnvilCatalogue(patch: string, locale: string): Promise<AnvilCatalogue> {
    const [map, table] = await Promise.all([
      this.#documents.anvilMap(patch),
      this.#documents.stringTable(patch, locale),
    ]);
    const { raw, names } = await parseAnvilDocuments(map, table);
    const built = anvilCatalogue(patch, locale, this.#documents.cdragonBase, raw, names);

    const counts = (["silver", "gold", "prismatic"] as const).map(
      (tier) => built.shards.filter((s) => s.tier === tier).length,
    );
    // A change is not an error (a patch can add a shard) but it is the moment to look at the pools
    // and the editor.
    if (counts.join() !== EXPECTED_ANVIL_POOLS.join()) {
      this.#log.warn({ patch, counts }, "stat anvil pools changed size");
    }
    const unnamed = raw.filter((r) => !names.has(r.nameKey)).length;
    if (unnamed > 0) {
      this.#log.warn({ patch, locale, unnamed }, "anvil shards with no name in this locale");
    }
    return built;
  }

  async #championDetails(dataPath: string, championId: number) {
    const raw = await this.#documents.championDetails(dataPath, championId);
    return readChampionDetails(parseJson(raw, `champion-details/${championId}.json`));
  }

  /** Reads a derived payload, building and storing it on a miss. */
  async #derived<T>(
    dataPath: string,
    kind: string,
    key: string,
    build: () => Promise<T>,
  ): Promise<T> {
    const body = this.#store.getDerived(dataPath, kind, key);
    if (body !== null) {
      try {
        return JSON.parse(body.toString("utf8")) as T;
      } catch (error) {
        // A row that no longer parses means it was written badly. Rebuild rather than fail, and say so.
        this.#log.warn({ kind, key, err: error }, "stale derived payload, rebuilding");
      }
    }

    const value = await build();
    this.#store.putDerived(dataPath, kind, key, Buffer.from(JSON.stringify(value), "utf8"));
    return value;
  }
}

function parseJson(raw: Buffer, what: string): unknown {
  try {
    return JSON.parse(raw.toString("utf8"));
  } catch (error) {
    throw new Error(`parsing ${what}: ${String(error)}`, { cause: error });
  }
}
