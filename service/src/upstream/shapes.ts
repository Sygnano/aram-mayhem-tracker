/**
 * The upstream JSON shapes, as observed on 16.19, and the readers that turn parsed JSON into them.
 *
 * Every reader is lenient: unknown fields are ignored, and a missing or mistyped field becomes its
 * default, so an aramkit schema change degrades a row rather than failing the whole response.
 */

// -- lenient readers -------------------------------------------------------------------------------

type Json = unknown;
type JsonObject = Record<string, Json>;

export function obj(value: Json): JsonObject {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as JsonObject)
    : {};
}

export function arr(value: Json): Json[] {
  return Array.isArray(value) ? value : [];
}

export function num(value: Json): number {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

/** An integer field. A fractional or missing value reads as 0, as serde's `i64` default did. */
export function int(value: Json): number {
  return Number.isInteger(value) ? (value as number) : 0;
}

export function str(value: Json): string {
  return typeof value === "string" ? value : "";
}

export function bool(value: Json): boolean {
  return value === true;
}

// -- aramkit ---------------------------------------------------------------------------------------

export interface ChampionDetails {
  champion: {
    id: number;
    stats: ChampionStatsDoc;
    tier: string;
    rank: number;
  };
  augments: AugmentRows;
  builds: { filtered: { archetypes: Archetype[] } };
  items: { filtered: { all: ItemStatDoc[] } };
}

export interface ChampionStatsDoc {
  sampleCount: number;
  winRate: number;
  pickRate: number;
  /** Average damage dealt to champions per game, by type. The three add up to `damageToChampions`. */
  physicalDamageToChampions: number;
  magicDamageToChampions: number;
  trueDamageToChampions: number;
}

/** `augments` of a champion's details, and the whole of `augment-rankings.json`. */
export interface AugmentRows {
  all: AugmentRow[];
  /** Keyed by stage number as a string: "1".."4". */
  stages: Record<string, AugmentRow[]>;
}

export interface AugmentRow {
  id: number;
  rank: number;
  tier: string;
  sampleCount: number;
  pickRate: number;
  /** How often *this champion* wins with the augment taken. Not the augment's own rate. */
  winRate: number;
  /** The augment's win rate across all champions. */
  augmentWinRate: number;
  availableStages: number[];
}

export interface Archetype {
  key: string;
  rank: number;
  sampleCount: number;
  pickRate: number;
  winRate: number;
  profiles: Profile[];
}

export interface Rates {
  rank: number;
  sampleCount: number;
  pickRate: number;
  winRate: number;
}

export interface Profile extends Rates {
  itemSet: number[];
  routes: Route[];
}

export interface Route extends Rates {
  purchaseOrder: number[];
  starters: (Rates & { items: number[] })[];
  boots: (Rates & { item: number })[];
  /** `null` where a later item names no item at all. */
  laterItems: (Rates & { itemId: number | null })[];
}

export interface ItemStatDoc extends Rates {
  id: number;
}

/**
 * `champion-rankings.json`: every champion aramkit has data for, in one 22 KB document. `rank` is
 * aramkit's own, and is **not** simply win-rate order (they weight it), so it is passed through.
 */
export interface ChampionRankingRow {
  id: number;
  rank: number;
  tier: string;
  sampleCount: number;
  /** A fraction, as everywhere upstream: 0.5778 is 57.78%. */
  winRate: number;
  pickRate: number;
}

function rates(value: Json): Rates {
  const o = obj(value);
  return {
    rank: int(o.rank),
    sampleCount: int(o.sampleCount),
    pickRate: num(o.pickRate),
    winRate: num(o.winRate),
  };
}

const itemId = (value: Json) => int(obj(value).id);

function augmentRow(value: Json): AugmentRow {
  const o = obj(value);
  return {
    id: int(o.id),
    rank: int(o.rank),
    tier: str(o.tier),
    sampleCount: int(o.sampleCount),
    pickRate: num(o.pickRate),
    winRate: num(o.winRate),
    augmentWinRate: num(o.augmentWinRate),
    availableStages: arr(o.availableStages).map(int),
  };
}

export function readAugmentRows(value: Json): AugmentRows {
  const o = obj(value);
  const stages: Record<string, AugmentRow[]> = {};
  for (const [stage, rows] of Object.entries(obj(o.stages))) {
    stages[stage] = arr(rows).map(augmentRow);
  }
  return { all: arr(o.all).map(augmentRow), stages };
}

function route(value: Json): Route {
  const o = obj(value);
  return {
    ...rates(o),
    purchaseOrder: arr(o.purchaseOrder).map(itemId),
    starters: arr(o.starters).map((s) => ({ ...rates(s), items: arr(obj(s).items).map(itemId) })),
    boots: arr(o.boots).map((b) => ({ ...rates(b), item: itemId(obj(b).item) })),
    laterItems: arr(o.laterItems).map((l) => {
      const later = obj(l);
      // An item is named by its own `id`, or by an `item` (or `itemRef`) object holding one.
      const ref = later.item ?? later.itemRef;
      const id = Number.isInteger(later.id)
        ? (later.id as number)
        : ref === undefined
          ? null
          : itemId(ref);
      return { ...rates(later), itemId: id };
    }),
  };
}

export function readChampionDetails(value: Json): ChampionDetails {
  const o = obj(value);
  const champion = obj(o.champion);
  const stats = obj(champion.stats);
  return {
    champion: {
      id: int(champion.id),
      stats: {
        sampleCount: int(stats.sampleCount),
        winRate: num(stats.winRate),
        pickRate: num(stats.pickRate),
        physicalDamageToChampions: num(stats.physicalDamageToChampions),
        magicDamageToChampions: num(stats.magicDamageToChampions),
        trueDamageToChampions: num(stats.trueDamageToChampions),
      },
      tier: str(champion.tier),
      rank: int(champion.rank),
    },
    augments: readAugmentRows(o.augments),
    builds: {
      filtered: {
        archetypes: arr(obj(obj(o.builds).filtered).archetypes).map((a) => {
          const archetype = obj(a);
          return {
            ...rates(archetype),
            key: str(archetype.key),
            profiles: arr(archetype.profiles).map((p) => {
              const profile = obj(p);
              return {
                ...rates(profile),
                itemSet: arr(profile.itemSet).map(itemId),
                routes: arr(profile.routes).map(route),
              };
            }),
          };
        }),
      },
    },
    items: {
      filtered: {
        all: arr(obj(obj(o.items).filtered).all).map((i) => ({ ...rates(i), id: itemId(i) })),
      },
    },
  };
}

export function readChampionRankings(value: Json): ChampionRankingRow[] {
  return arr(obj(value).rows).map((r) => {
    const o = obj(r);
    return {
      id: int(o.id),
      rank: int(o.rank),
      tier: str(o.tier),
      sampleCount: int(o.sampleCount),
      winRate: num(o.winRate),
      pickRate: num(o.pickRate),
    };
  });
}

// -- CommunityDragon -------------------------------------------------------------------------------

/** A row of `cherry-augments.json`. Mirrors `static-data`'s type in the desktop app. */
export interface CherryAugment {
  id: number;
  augmentNameId: string;
  nameTRA: string;
  augmentSmallIconPath: string;
  rarity: string;
}

export interface AugmentList {
  modeName: string;
  augmentList: string[];
}

export function readCherryAugments(value: Json): CherryAugment[] {
  return arr(value).map((a) => {
    const o = obj(a);
    return {
      id: int(o.id),
      augmentNameId: str(o.augmentNameId),
      nameTRA: str(o.nameTRA),
      augmentSmallIconPath: str(o.augmentSmallIconPath),
      rarity: str(o.rarity),
    };
  });
}

export function readAugmentLists(value: Json): AugmentList[] {
  return arr(value).map((l) => {
    const o = obj(l);
    return { modeName: str(o.modeName), augmentList: arr(o.augmentList).map(str) };
  });
}

/**
 * `kSilver` / `kGold` / `kPrismatic` → `silver` / `gold` / `prismatic`, matching the rarity strings
 * the vision crate produces from the card frames.
 */
export function normaliseRarity(raw: string): string {
  return raw.trim().replace(/^k+/, "").toLowerCase();
}

/** CommunityDragon icon paths are absolute game paths; this maps one to a fetchable URL. */
export function iconUrl(base: string, patch: string, iconPath: string): string {
  const lowered = iconPath.replace(/^\/+/, "").toLowerCase();
  const tail = lowered.startsWith("lol-game-data/assets/")
    ? lowered.slice("lol-game-data/assets/".length)
    : lowered;
  return `${base}/${patch}/plugins/rcp-be-lol-game-data/global/default/${tail}`;
}
