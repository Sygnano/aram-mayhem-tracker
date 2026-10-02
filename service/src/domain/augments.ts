/**
 * Turning upstream augment documents into the tables the endpoints serve from.
 *
 * The two decisions that live here: rank augments by their win-rate delta against the
 * champion's own baseline, and fall back stage → champion → global with a 200-game floor, reporting
 * which rung was used rather than presenting thin data as fact.
 *
 * Tables keyed by id are plain objects, so they survive the JSON round trip through the `derived`
 * cache unchanged.
 */

import {
  type AugmentList,
  type AugmentRow,
  type AugmentRows,
  type ChampionDetails,
  type ChampionRankingRow,
  type CherryAugment,
  iconUrl,
  normaliseRarity,
} from "../upstream/shapes.ts";

/**
 * Minimum games before a row is trusted to drive the ranking (the owner's call). Rows below
 * this are still shown, flagged `lowSample`, rather than hidden.
 */
export const MIN_SAMPLES = 200;

/** The four Mayhem offers, at levels 1, 7, 11 and 15. */
export const STAGES = [1, 2, 3, 4] as const;

// -- derived documents (what we cache) --------------------------------------------------------------

export interface Row {
  winRate: number;
  pickRate: number;
  sampleCount: number;
  tier: string;
  rank: number;
  augmentWinRate: number;
}

export interface AugmentEntry {
  all: Row | null;
  /** Stage number → row, for the stages this augment appears in. */
  stages: Record<number, Row>;
  availableStages: number[];
}

export interface ChampionStats {
  id: number;
  winRate: number;
  pickRate: number;
  sampleCount: number;
  tier: string;
  rank: number;
}

/** One champion's augment table, keyed by Riot augment id. */
export interface ChampionAugments {
  champion: ChampionStats;
  augments: Record<number, AugmentEntry>;
}

/**
 * Every champion's standing for one aramkit build: the table champ select reads.
 *
 * `poolSize` is what a rank is *out of*, and it comes from the document's own row count rather than
 * from a constant: "#3 / 173" is a different claim from "#3 / 168", and the number of ranked
 * champions moves with each patch.
 */
export interface ChampionTable {
  poolSize: number;
  /** Keyed by Riot champion id. */
  champions: Record<number, ChampionStats>;
}

/** Augment metadata from CommunityDragon: rarity, name and icon. */
export interface AugmentMeta {
  id: number;
  nameId: string;
  name: string;
  /** `silver` / `gold` / `prismatic`, matching what the vision crate reads off the card frames. */
  rarity: string;
  iconUrl: string;
}

export interface AugmentCatalogue {
  patch: string;
  byId: Record<number, AugmentMeta>;
  /** Riot augment ids in the `KIWI` (ARAM: Mayhem) pool. */
  kiwi: number[];
  /** Pool entries that did not resolve to an augment. Research found zero on 16.19. */
  unmatched: string[];
}

// -- parsing -----------------------------------------------------------------------------------------

function toRow(r: AugmentRow): Row {
  return {
    winRate: r.winRate,
    pickRate: r.pickRate,
    sampleCount: r.sampleCount,
    tier: r.tier,
    rank: r.rank,
    augmentWinRate: r.augmentWinRate,
  };
}

/** A stage key as aramkit writes it, "1".."4", or `null` for anything that is not a stage number. */
function stageNumber(key: string): number | null {
  if (!/^\+?\d+$/.test(key)) {
    return null;
  }
  const stage = Number(key);
  return stage <= 255 ? stage : null;
}

/** The per-augment table shared by a champion's details and the champion-agnostic rankings. */
function augmentTable(rows: AugmentRows): Record<number, AugmentEntry> {
  const out: Record<number, AugmentEntry> = {};
  const entry = (id: number) => {
    out[id] ??= { all: null, stages: {}, availableStages: [] };
    return out[id];
  };

  for (const row of rows.all) {
    const e = entry(row.id);
    e.all = toRow(row);
    e.availableStages = [...row.availableStages];
  }
  for (const [key, stageRows] of Object.entries(rows.stages)) {
    const stage = stageNumber(key);
    if (stage === null) {
      continue;
    }
    for (const row of stageRows) {
      entry(row.id).stages[stage] = toRow(row);
    }
  }
  return out;
}

export function championAugments(details: ChampionDetails): ChampionAugments {
  const { champion } = details;
  return {
    champion: {
      id: champion.id,
      winRate: champion.stats.winRate,
      pickRate: champion.stats.pickRate,
      sampleCount: champion.stats.sampleCount,
      tier: champion.tier,
      rank: champion.rank,
    },
    augments: augmentTable(details.augments),
  };
}

/** The champion-agnostic table, used when a champion has no usable row for an augment. */
export function globalRankings(rankings: AugmentRows): Record<number, AugmentEntry> {
  return augmentTable(rankings);
}

/**
 * Turns `champion-rankings.json` into a table keyed by champion id.
 *
 * Rows with no id are dropped (a row we cannot attribute is worse than a missing one) and
 * `poolSize` counts the rows kept, so it always matches what can actually be looked up.
 */
export function championRankingsTable(rows: readonly ChampionRankingRow[]): ChampionTable {
  const champions: Record<number, ChampionStats> = {};
  for (const r of rows) {
    if (r.id > 0) {
      champions[r.id] = {
        id: r.id,
        winRate: r.winRate,
        pickRate: r.pickRate,
        sampleCount: r.sampleCount,
        tier: r.tier,
        rank: r.rank,
      };
    }
  }
  return { poolSize: Object.keys(champions).length, champions };
}

export function catalogue(
  patch: string,
  cdragonBase: string,
  augments: readonly CherryAugment[],
  lists: readonly AugmentList[],
): AugmentCatalogue {
  const byName = new Map<string, CherryAugment>();
  for (const a of augments) {
    if (a.augmentNameId !== "") {
      byName.set(a.augmentNameId, a);
    }
  }

  const kiwi: number[] = [];
  const unmatched: string[] = [];
  const list = lists.find((l) => l.modeName === "KIWI");
  for (const entry of list?.augmentList ?? []) {
    const nameId = entry.slice(entry.lastIndexOf("/") + 1);
    const augment = byName.get(nameId);
    if (augment) {
      kiwi.push(augment.id);
    } else {
      unmatched.push(nameId);
    }
  }

  const byId: Record<number, AugmentMeta> = {};
  for (const a of augments) {
    byId[a.id] = {
      id: a.id,
      nameId: a.augmentNameId,
      name: a.nameTRA,
      rarity: normaliseRarity(a.rarity),
      iconUrl: iconUrl(cdragonBase, patch, a.augmentSmallIconPath),
    };
  }

  return { patch, byId, kiwi, unmatched };
}

// -- the metric and the ladder -------------------------------------------------------------------

/**
 * Which rung of the fallback ladder a number came from: `stage` is this champion at this stage,
 * `champion` this champion with all stages pooled, `global` every champion (the augment's own rate,
 * not comparable to the champion-relative numbers), and `none` an augment upstream has never seen.
 */
export type Source = "stage" | "champion" | "global" | "none";

/**
 * `high` is at or above the 200-game floor and `low` below it, shown with a warning. `fallback` is
 * champion-agnostic: its delta is measured against the 50% population average, so it answers a
 * different question.
 */
export type Confidence = "high" | "low" | "fallback" | "none";

/** What the ladder settled on for one augment, ready to serve. */
export interface Resolved {
  winRate: number | null;
  /** Percentage points above or below the baseline named by `deltaBasis`. */
  deltaPp: number | null;
  pickRate: number | null;
  augmentWinRate: number | null;
  sampleCount: number;
  tier: string | null;
  rank: number | null;
  source: Source;
  confidence: Confidence;
  lowSample: boolean;
  /**
   * `champion` when the delta is against this champion's own win rate, `global` when it is against
   * the 50% population average.
   */
  deltaBasis: "champion" | "global" | "none";
}

function missing(): Resolved {
  return {
    winRate: null,
    deltaPp: null,
    pickRate: null,
    augmentWinRate: null,
    sampleCount: 0,
    tier: null,
    rank: null,
    source: "none",
    confidence: "none",
    lowSample: true,
    deltaBasis: "none",
  };
}

function fromRow(
  row: Row,
  baseline: number,
  source: Source,
  deltaBasis: Resolved["deltaBasis"],
): Resolved {
  const lowSample = row.sampleCount < MIN_SAMPLES;
  return {
    winRate: row.winRate,
    deltaPp: (row.winRate - baseline) * 100,
    pickRate: row.pickRate,
    augmentWinRate: row.augmentWinRate > 0 ? row.augmentWinRate : null,
    sampleCount: row.sampleCount,
    tier: row.tier === "" ? null : row.tier,
    rank: row.rank > 0 ? row.rank : null,
    source,
    confidence: source === "global" ? "fallback" : lowSample ? "low" : "high",
    lowSample,
    deltaBasis,
  };
}

/**
 * Walks the ladder for one augment. `stage` is aramkit's 1–4, the offers at levels 1,
 * 7, 11 and 15.
 */
export function resolve(
  entry: AugmentEntry | undefined,
  global: AugmentEntry | undefined,
  stage: number | null,
  championWinRate: number,
): Resolved {
  if (entry) {
    const stageRow = stage === null ? undefined : entry.stages[stage];
    // Rung 1: this champion at this stage, if it clears the floor.
    if (stageRow && stageRow.sampleCount >= MIN_SAMPLES) {
      return fromRow(stageRow, championWinRate, "stage", "champion");
    }
    // Rung 2: this champion, all stages pooled.
    if (entry.all && entry.all.sampleCount >= MIN_SAMPLES) {
      return fromRow(entry.all, championWinRate, "champion", "champion");
    }
    // Still nothing above the floor: prefer the most specific thin row we have, flagged, over
    // silently switching to a champion-agnostic number.
    if (stageRow) {
      return fromRow(stageRow, championWinRate, "stage", "champion");
    }
    if (entry.all) {
      return fromRow(entry.all, championWinRate, "champion", "champion");
    }
  }

  // Rung 3: the champion-agnostic table. Measured against the population average, because there is
  // no champion in this number to compare against.
  if (global) {
    const row = (stage === null ? undefined : global.stages[stage]) ?? global.all;
    if (row) {
      return fromRow(row, 0.5, "global", "global");
    }
  }

  return missing();
}
