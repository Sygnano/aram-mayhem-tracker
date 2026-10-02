/** How champions split their damage to champions between physical, magic and true. */

import type { ChampionDetails } from "../upstream/shapes.ts";

/**
 * One champion's damage split in ARAM: Mayhem.
 *
 * Measured, not classified: it is aramkit's average over every game it has for the champion, so it
 * reflects the builds and augments people actually take. That also makes it one average across all
 * of those builds: a champion built two ways gets the blend, not either build.
 */
export interface DamageProfile {
  id: number;
  /** Fractions of the champion's damage to champions. With `magic` and `trueDamage` they sum to 1. */
  physical: number;
  magic: number;
  trueDamage: number;
  /** Average damage to champions per game: the weight to use when adding champions into a team. */
  damagePerGame: number;
  sampleCount: number;
}

/**
 * The damage split from a champion's details, or `null` when upstream reports no damage at all,
 * which is a champion it has no games for rather than one that deals none.
 */
export function damageProfile(details: ChampionDetails): DamageProfile | null {
  const { stats } = details.champion;
  const physical = Math.max(stats.physicalDamageToChampions, 0);
  const magic = Math.max(stats.magicDamageToChampions, 0);
  const trueDamage = Math.max(stats.trueDamageToChampions, 0);
  // Shares of the three parts' own sum, not of upstream's total: the parts are rounded separately
  // upstream, and this way the shares always sum to 1.
  const total = physical + magic + trueDamage;
  if (total <= 0) {
    return null;
  }
  return {
    id: details.champion.id,
    physical: physical / total,
    magic: magic / total,
    trueDamage: trueDamage / total,
    damagePerGame: total,
    sampleCount: stats.sampleCount,
  };
}

/**
 * A team's damage split: its champions' profiles added up, each weighted by how much damage that
 * champion deals in a game. A plain average of the shares would let a support count for as much as
 * the carry.
 */
export function teamDamage(champions: readonly DamageProfile[]): DamageProfile | null {
  const total = champions.reduce((sum, c) => sum + c.damagePerGame, 0);
  if (total <= 0) {
    return null;
  }
  const share = (part: (c: DamageProfile) => number) =>
    champions.reduce((sum, c) => sum + part(c) * c.damagePerGame, 0) / total;
  return {
    id: 0,
    physical: share((c) => c.physical),
    magic: share((c) => c.magic),
    trueDamage: share((c) => c.trueDamage),
    damagePerGame: total,
    sampleCount: champions.length === 0 ? 0 : Math.min(...champions.map((c) => c.sampleCount)),
  };
}
