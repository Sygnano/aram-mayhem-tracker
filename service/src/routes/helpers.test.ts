import { describe, expect, it } from "vitest";
import { STAGES } from "../domain/augments.ts";
import { levelForStage, parseIds, sortBestFirst, validatedStage } from "./augments.ts";
import { parseChampionIds } from "./champions.ts";

type Source = "stage" | "champion" | "global" | "none";
const info = (id: number, source: Source, deltaPp: number | null) => ({ id, source, deltaPp });

describe("ranking augments", () => {
  it("never lets a global fallback outrank champion-specific data", () => {
    // The real case that surfaced this: Yasuo has no row for Draw Your Sword (1134), so it falls
    // back to the global table and reads +9.96pp against the 50% average, a bigger number than
    // Mystic Punch's +8.88pp against Yasuo's own 57.73%, but not a better augment for Yasuo.
    const augments = sortBestFirst([
      info(1134, "global", 9.96),
      info(1058, "stage", 8.88),
      info(2031, "stage", 4.0),
    ]);
    expect(augments.map((a) => a.id)).toEqual([1058, 2031, 1134]);
  });

  it("sorts augments with no data last", () => {
    const augments = sortBestFirst([
      info(1, "none", null),
      info(2, "global", -5.0),
      info(3, "champion", -9.0),
    ]);
    expect(augments.map((a) => a.id)).toEqual([3, 2, 1]);
  });
});

describe("query parameters", () => {
  it("map stages to the levels the game uses", () => {
    expect(STAGES.map(levelForStage)).toEqual([1, 7, 11, 15]);
  });

  it("parse and bound augment ids", () => {
    expect(parseIds("1058,1134, 2031")).toEqual([1058, 1134, 2031]);
    expect(() => parseIds("")).toThrow();
    expect(() => parseIds("1058,abc")).toThrow();
    expect(() => parseIds(Array(13).fill("1").join(","))).toThrow();
  });

  it("parse, deduplicate and bound champion ids", () => {
    expect(parseChampionIds("157, 103,157")).toEqual([157, 103]);
    expect(() => parseChampionIds("")).toThrow();
    expect(() => parseChampionIds("157,yasuo")).toThrow();
    expect(() => parseChampionIds("0")).toThrow();
    const eleven = Array.from({ length: 11 }, (_, i) => String(i + 1));
    expect(() => parseChampionIds(eleven.join(","))).toThrow();
  });

  it("accept only real stages", () => {
    expect(validatedStage(4)).toBe(4);
    expect(validatedStage(undefined)).toBeNull();
    expect(() => validatedStage(0)).toThrow();
    expect(() => validatedStage(5)).toThrow();
  });
});
