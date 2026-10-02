/**
 * `/v1/offer` and `/v1/pool`: augments for one champion, ranked by win-rate delta.
 *
 * Responses are small on purpose: an offer is about a kilobyte against 461 KB of raw upstream.
 */

import type { FastifyPluginAsyncTypebox } from "@fastify/type-provider-typebox";
import { type Static, Type } from "typebox";
import type { Data } from "../data.ts";
import type { Store } from "../db/store.ts";
import {
  type AugmentCatalogue,
  type AugmentEntry,
  type ChampionAugments,
  MIN_SAMPLES,
  type Resolved,
  resolve,
  STAGES,
} from "../domain/augments.ts";
import { BadRequestError } from "../errors.ts";
import { knownChampion } from "./champions.ts";
import { ChampionInfo, Nullable, PatchFields } from "./schemas.ts";
import { currentVersion } from "./status.ts";

export interface AugmentRouteOptions {
  store: Store;
  data: Data;
}

// -- wire types ------------------------------------------------------------------------------------

const StageInfo = Type.Object({
  /** aramkit's stage number, 1–4. */
  stage: Type.Integer(),
  /** The in-game level the stage corresponds to: 1, 7, 11, 15. */
  level: Type.Integer(),
  winRate: Type.Number(),
  deltaPp: Type.Number(),
  pickRate: Type.Number(),
  sampleCount: Type.Integer(),
  lowSample: Type.Boolean(),
});

/** One augment as the overlay shows it: its metadata, then what the ladder settled on. */
export const AugmentInfo = Type.Object({
  id: Type.Integer(),
  nameId: Nullable(Type.String()),
  name: Nullable(Type.String()),
  rarity: Nullable(Type.String()),
  iconUrl: Nullable(Type.String()),

  winRate: Nullable(Type.Number()),
  deltaPp: Nullable(Type.Number()),
  pickRate: Nullable(Type.Number()),
  augmentWinRate: Nullable(Type.Number()),
  sampleCount: Type.Integer(),
  tier: Nullable(Type.String()),
  rank: Nullable(Type.Integer()),
  source: Type.Union([
    Type.Literal("stage"),
    Type.Literal("champion"),
    Type.Literal("global"),
    Type.Literal("none"),
  ]),
  confidence: Type.Union([
    Type.Literal("high"),
    Type.Literal("low"),
    Type.Literal("fallback"),
    Type.Literal("none"),
  ]),
  lowSample: Type.Boolean(),
  deltaBasis: Type.Union([Type.Literal("champion"), Type.Literal("global"), Type.Literal("none")]),

  /**
   * The stages this augment can be offered at, as aramkit reports them. An augment that only appears
   * from level 11 lists [3, 4]. Empty when upstream does not say.
   */
  availableStages: Type.Array(Type.Integer()),
  /**
   * Every stage we have a number for, whatever its sample size, so the app can show win rate by
   * stage with its own warning on thin ones.
   */
  byStage: Type.Array(StageInfo),
});

const OfferResponse = Type.Object({
  ...PatchFields,
  champion: ChampionInfo,
  stage: Nullable(Type.Integer()),
  augments: Type.Array(AugmentInfo),
  /** Augment ids best first, by delta. The overlay highlights `ranking[0]`. */
  ranking: Type.Array(Type.Integer()),
});

const PoolResponse = Type.Object({
  ...PatchFields,
  champion: ChampionInfo,
  rarity: Type.String(),
  stage: Nullable(Type.Integer()),
  augments: Type.Array(AugmentInfo),
});

const OfferQuery = Type.Object({
  champion: Type.Integer(),
  stage: Type.Optional(Type.Integer()),
  /** Comma-separated Riot augment ids, as read off the cards. */
  augments: Type.String(),
});

const PoolQuery = Type.Object({
  champion: Type.Integer(),
  rarity: Type.String(),
  stage: Type.Optional(Type.Integer()),
});

type AugmentInfo = Static<typeof AugmentInfo>;

// -- handlers --------------------------------------------------------------------------------------

export const augmentRoutes: FastifyPluginAsyncTypebox<AugmentRouteOptions> = async (
  app,
  { store, data },
) => {
  app.get(
    "/v1/offer",
    { schema: { querystring: OfferQuery, response: { 200: OfferResponse } } },
    async ({ query, log }) => {
      const version = currentVersion(store);
      const stage = validatedStage(query.stage);
      const ids = parseIds(query.augments);
      await knownChampion(data, version.dataPath, query.champion, log);

      const [champion, global, catalogue] = await Promise.all([
        data.championAugments(version.dataPath, query.champion),
        data.globalRankings(version.dataPath),
        data.catalogue(version.version),
      ]);

      const augments = sortBestFirst(
        ids.map((id) => buildInfo(id, champion, global, catalogue, stage)),
      );
      return {
        patch: version.version,
        dataDate: version.dataDate,
        champion: championInfo(champion),
        stage,
        augments,
        ranking: augments.map((a) => a.id),
      };
    },
  );

  app.get(
    "/v1/pool",
    { schema: { querystring: PoolQuery, response: { 200: PoolResponse } } },
    async ({ query, log }) => {
      const version = currentVersion(store);
      const stage = validatedStage(query.stage);
      const rarity = query.rarity.trim().toLowerCase();
      if (!["silver", "gold", "prismatic"].includes(rarity)) {
        throw new BadRequestError(
          `rarity must be silver, gold or prismatic, not ${JSON.stringify(query.rarity)}`,
        );
      }
      await knownChampion(data, version.dataPath, query.champion, log);

      const [champion, global, catalogue] = await Promise.all([
        data.championAugments(version.dataPath, query.champion),
        data.globalRankings(version.dataPath),
        data.catalogue(version.version),
      ]);

      const augments = poolAugments(rarity, stage, champion, global, catalogue);
      return {
        patch: version.version,
        dataDate: version.dataDate,
        champion: championInfo(champion),
        rarity,
        stage,
        augments,
      };
    },
  );
};

// -- helpers ---------------------------------------------------------------------------------------

/** 0 for champion-relative numbers, 1 for champion-agnostic ones, 2 for no data. */
function baselineTier(source: Resolved["source"]): number {
  return source === "global" ? 1 : source === "none" ? 2 : 0;
}

/**
 * Orders augments best first.
 *
 * Deltas from different baselines are not comparable, so the ordering is by baseline tier first and
 * only then by delta. A champion-agnostic row measured against the 50% population average can read
 * as a large delta (Draw Your Sword is +9.96pp globally) and would otherwise outrank a genuinely
 * better augment measured against the champion's own win rate. Augments with no data at all sort
 * last rather than winning by default.
 */
export function sortBestFirst<T extends Pick<AugmentInfo, "source" | "deltaPp">>(
  augments: T[],
): T[] {
  const delta = (a: T) => a.deltaPp ?? Number.NEGATIVE_INFINITY;
  return augments.sort((a, b) => {
    const tier = baselineTier(a.source) - baselineTier(b.source);
    if (tier !== 0) {
      return tier;
    }
    const [x, y] = [delta(a), delta(b)];
    return x > y ? -1 : x < y ? 1 : 0;
  });
}

export function validatedStage(stage: number | undefined): number | null {
  if (stage === undefined) {
    return null;
  }
  if ((STAGES as readonly number[]).includes(stage)) {
    return stage;
  }
  throw new BadRequestError(`stage must be 1, 2, 3 or 4 (levels 1, 7, 11, 15), not ${stage}`);
}

/** Comma-separated integer ids, blanks skipped. `null` if any part is not an integer. */
export function splitIds(raw: string): number[] | null {
  const parts = raw
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s !== "");
  if (!parts.every((s) => /^[+-]?\d+$/.test(s))) {
    return null;
  }
  const ids = parts.map(Number);
  return ids.every(Number.isSafeInteger) ? ids : null;
}

export function parseIds(raw: string): number[] {
  const ids = splitIds(raw);
  if (ids === null) {
    throw new BadRequestError(`augments must be comma-separated ids, got ${JSON.stringify(raw)}`);
  }
  if (ids.length === 0) {
    throw new BadRequestError("no augment ids given");
  }
  if (ids.length > 12) {
    // Three cards, each rerollable; a dozen is already generous.
    throw new BadRequestError(`too many augment ids (${ids.length})`);
  }
  return ids;
}

/**
 * Every augment of one rarity, ranked for one champion at one stage. Pool membership comes from
 * CommunityDragon, so an augment aramkit has never seen still appears in the list, marked as having
 * no data.
 */
export function poolAugments(
  rarity: string,
  stage: number | null,
  champion: ChampionAugments,
  global: Record<number, AugmentEntry>,
  catalogue: AugmentCatalogue,
): AugmentInfo[] {
  return sortBestFirst(
    catalogue.kiwi
      .filter((id) => catalogue.byId[id]?.rarity === rarity)
      .map((id) => buildInfo(id, champion, global, catalogue, stage)),
  );
}

export function championInfo({ champion }: ChampionAugments) {
  return {
    id: champion.id,
    winRate: champion.winRate,
    pickRate: champion.pickRate,
    sampleCount: champion.sampleCount,
    tier: champion.tier === "" ? null : champion.tier,
  };
}

/** The in-game level an aramkit stage corresponds to. */
export function levelForStage(stage: number): number {
  return stage === 1 ? 1 : stage === 2 ? 7 : stage === 3 ? 11 : 15;
}

function buildInfo(
  id: number,
  champion: ChampionAugments,
  global: Record<number, AugmentEntry>,
  catalogue: AugmentCatalogue,
  stage: number | null,
): AugmentInfo {
  const entry = champion.augments[id];
  const globalEntry = global[id];
  const championWinRate = champion.champion.winRate;
  const meta = catalogue.byId[id];

  // Per-stage numbers are reported as they are, thin ones included but flagged, because the owner
  // asked to see low sample counts with a warning rather than have them disappear.
  const byStage = Object.entries(entry?.stages ?? {})
    .map(([key, row]) => {
      const s = Number(key);
      return {
        stage: s,
        level: levelForStage(s),
        winRate: row.winRate,
        deltaPp: (row.winRate - championWinRate) * 100,
        pickRate: row.pickRate,
        sampleCount: row.sampleCount,
        lowSample: row.sampleCount < MIN_SAMPLES,
      };
    })
    .sort((a, b) => a.stage - b.stage);

  const availableStages =
    entry && entry.availableStages.length > 0
      ? entry.availableStages
      : (globalEntry?.availableStages ?? []);

  return {
    id,
    nameId: meta?.nameId ?? null,
    name: meta?.name ?? null,
    rarity: meta?.rarity ?? null,
    iconUrl: meta?.iconUrl ?? null,
    ...resolve(entry, globalEntry, stage, championWinRate),
    availableStages,
    byStage,
  };
}
