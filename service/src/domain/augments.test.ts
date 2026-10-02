import { describe, expect, it } from "vitest";
import type { ChampionRankingRow } from "../upstream/shapes.ts";
import {
  type AugmentEntry,
  championRankingsTable,
  MIN_SAMPLES,
  type Row,
  resolve,
} from "./augments.ts";

function row(winRate: number, sampleCount: number): Row {
  return { winRate, pickRate: 0.1, sampleCount, tier: "A", rank: 3, augmentWinRate: 0.51 };
}

function entry(all: Row | null, stages: [number, Row][] = []): AugmentEntry {
  return { all, stages: Object.fromEntries(stages), availableStages: [1, 2, 3, 4] };
}

describe("the fallback ladder", () => {
  it("measures the delta against the champion baseline", () => {
    // The real Yasuo numbers: 65.13% with an augment, 57.73% overall, so +7.4pp.
    const r = resolve(entry(row(0.6513, 320_503)), undefined, 2, 0.5773);

    expect(r.source).toBe("champion");
    expect(r.confidence).toBe("high");
    expect(r.deltaPp).toBeCloseTo(7.4, 2);
    expect(r.deltaBasis).toBe("champion");
    expect(r.lowSample).toBe(false);
  });

  it("prefers a stage row above the floor", () => {
    const e = entry(row(0.6, 50_000), [[2, row(0.7, MIN_SAMPLES)]]);
    const r = resolve(e, undefined, 2, 0.55);
    expect(r.source).toBe("stage");
    expect(r.winRate).toBe(0.7);
  });

  it("falls back from a thin stage row to the champion row", () => {
    // The 60-sample stage rows that real data is full of must not drive the ranking.
    const e = entry(row(0.6, 50_000), [[2, row(0.95, 60)]]);
    const r = resolve(e, undefined, 2, 0.55);

    expect(r.source).toBe("champion");
    expect(r.winRate).toBe(0.6);
    expect(r.confidence).toBe("high");
  });

  it("shows thin data with a warning rather than hiding it", () => {
    const r = resolve(entry(null, [[2, row(0.95, 60)]]), undefined, 2, 0.55);

    expect(r.source).toBe("stage");
    expect(r.lowSample).toBe(true);
    expect(r.confidence).toBe("low");
    expect(r.winRate, "the real number is still reported").toBe(0.95);
  });

  it("falls back to the global table for an augment the champion has no row for", () => {
    const r = resolve(undefined, entry(row(0.585, 2_762_899)), 1, 0.5773);

    expect(r.source).toBe("global");
    expect(r.confidence).toBe("fallback");
    expect(r.deltaBasis).toBe("global");
    // Against the 50% population average, not against the champion.
    expect(r.deltaPp).toBeCloseTo(8.5, 2);
  });

  it("reports an augment upstream has never seen as missing", () => {
    const r = resolve(undefined, undefined, 1, 0.5773);
    expect(r.source).toBe("none");
    expect(r.winRate).toBeNull();
    expect(r.deltaPp).toBeNull();
  });
});

function rankingRow(id: number, rank: number, tier: string, winRate: number): ChampionRankingRow {
  return { id, rank, tier, sampleCount: 200_000, winRate, pickRate: 0.01 };
}

describe("the champion table", () => {
  it("is keyed by id and counts its own rows", () => {
    const table = championRankingsTable([
      rankingRow(157, 1, "S", 0.5778),
      rankingRow(432, 3, "D", 0.402),
      rankingRow(103, 2, "B", 0.51),
    ]);

    expect(table.poolSize, "what a rank is out of, from the rows themselves").toBe(3);
    expect(table.champions[157]?.rank).toBe(1);
    expect(table.champions[157]?.tier).toBe("S");
    expect(table.champions[432]?.winRate, "a fraction, not a percentage").toBe(0.402);
  });

  /**
   * A row we cannot attribute to a champion is dropped, and `poolSize` counts what survived;
   * otherwise a rank would be measured against rows nobody can look up.
   */
  it("drops an unattributable row and does not count it", () => {
    const table = championRankingsTable([
      rankingRow(157, 1, "S", 0.58),
      rankingRow(0, 2, "C", 0.49),
    ]);

    expect(Object.keys(table.champions)).toHaveLength(1);
    expect(table.poolSize, "the dropped row does not inflate the denominator").toBe(1);
  });

  /**
   * aramkit's rank is not win-rate order (they weight it), so it must be passed through rather than
   * recomputed, or a champion's rank would contradict the tier shown beside it.
   */
  it("passes aramkit's rank through even when it contradicts win-rate order", () => {
    const table = championRankingsTable([rankingRow(1, 2, "A", 0.56), rankingRow(2, 1, "S", 0.55)]);

    expect(table.champions[2]?.rank, "ranked first despite the lower win rate").toBe(1);
    expect(table.champions[1]?.rank).toBe(2);
  });
});
