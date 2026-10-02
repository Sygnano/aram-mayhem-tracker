/**
 * Item sets, one per archetype.
 *
 * Each archetype's best profile and best route give the four blocks the app turns into an LCU item
 * set: starters, boots, core (in purchase order) and situational.
 */

import type { ChampionDetails } from "../upstream/shapes.ts";
import { MIN_SAMPLES } from "./augments.ts";

/** How many entries each block keeps. The LCU renders these in game, so a block of forty is useless. */
const MAX_STARTERS = 3;
const MAX_BOOTS = 3;
const MAX_SITUATIONAL = 8;

export interface ItemStat {
  id: number;
  winRate: number;
  pickRate: number;
  sampleCount: number;
}

/** A starter block is a set of items bought together, not a single item. */
export interface StarterSet {
  items: number[];
  winRate: number;
  pickRate: number;
  sampleCount: number;
}

export interface ArchetypeBuild {
  /** aramkit's own key: `crit`, `bruiser`, `ad`, `on_hit`, `tank`… */
  key: string;
  rank: number;
  winRate: number;
  pickRate: number;
  sampleCount: number;
  starters: StarterSet[];
  boots: ItemStat[];
  /** The core items in the order they are actually bought. */
  core: number[];
  situational: ItemStat[];
}

/** A rank of 0 is missing, and sorts last rather than first. */
const rankKey = (rank: number) => (rank > 0 ? rank : Number.POSITIVE_INFINITY);

/** The entry with the lowest rank; on a tie, the first. */
function best<T extends { rank: number }>(items: readonly T[]): T | undefined {
  let found: T | undefined;
  for (const item of items) {
    if (found === undefined || rankKey(item.rank) < rankKey(found.rank)) {
      found = item;
    }
  }
  return found;
}

const byPickRateDesc = (a: { pickRate: number }, b: { pickRate: number }) =>
  b.pickRate - a.pickRate;

/** Builds one entry per archetype, best first. */
export function archetypes(details: ChampionDetails): ArchetypeBuild[] {
  const out: ArchetypeBuild[] = [];

  for (const arch of details.builds.filtered.archetypes) {
    // aramkit ranks these itself; rank 1 is the most-played profile and route.
    const profile = best(arch.profiles);
    if (!profile) {
      continue;
    }
    const route = best(profile.routes);

    // The purchase order is the same items as `itemSet`, but ordered; fall back to the set when a
    // profile has no routes at all.
    const core =
      route && route.purchaseOrder.length > 0 ? [...route.purchaseOrder] : [...profile.itemSet];

    const starters: StarterSet[] = (route?.starters ?? [])
      .map((s) => ({
        items: [...s.items],
        winRate: s.winRate,
        pickRate: s.pickRate,
        sampleCount: s.sampleCount,
      }))
      .filter((s) => s.items.length > 0)
      .sort(byPickRateDesc)
      .slice(0, MAX_STARTERS);

    const boots: ItemStat[] = (route?.boots ?? [])
      .map((b) => ({
        id: b.item,
        winRate: b.winRate,
        pickRate: b.pickRate,
        sampleCount: b.sampleCount,
      }))
      .filter((b) => b.id > 0)
      .sort(byPickRateDesc)
      .slice(0, MAX_BOOTS);

    // Situational: the route's later items first, then anything else with a strong win rate that is
    // not already in the set, so the block is useful rather than a repeat.
    const placed = new Set<number>([
      ...core,
      ...boots.map((b) => b.id),
      ...starters.flatMap((s) => s.items),
    ]);
    const situational: ItemStat[] = [];
    for (const later of route?.laterItems ?? []) {
      if (later.itemId !== null && !placed.has(later.itemId)) {
        placed.add(later.itemId);
        situational.push({
          id: later.itemId,
          winRate: later.winRate,
          pickRate: later.pickRate,
          sampleCount: later.sampleCount,
        });
      }
    }
    const extras = details.items.filtered.all
      .filter((i) => i.id > 0 && !placed.has(i.id) && i.sampleCount >= MIN_SAMPLES)
      .sort((a, b) => b.winRate - a.winRate);
    for (const item of extras) {
      if (situational.length >= MAX_SITUATIONAL) {
        break;
      }
      if (!placed.has(item.id)) {
        placed.add(item.id);
        situational.push({
          id: item.id,
          winRate: item.winRate,
          pickRate: item.pickRate,
          sampleCount: item.sampleCount,
        });
      }
    }

    out.push({
      key: arch.key,
      rank: arch.rank,
      winRate: arch.winRate,
      pickRate: arch.pickRate,
      sampleCount: arch.sampleCount,
      starters,
      boots,
      core,
      situational: situational.slice(0, MAX_SITUATIONAL),
    });
  }

  return out.sort((a, b) => {
    const [x, y] = [rankKey(a.rank), rankKey(b.rank)];
    return x < y ? -1 : x > y ? 1 : 0;
  });
}
