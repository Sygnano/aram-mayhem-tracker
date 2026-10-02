import { describe, expect, it } from "vitest";
import { readChampionDetails } from "../upstream/shapes.ts";
import { MIN_SAMPLES } from "./augments.ts";
import { archetypes } from "./builds.ts";

const item = (id: number) => ({ id });

/** The upstream shape, so the reader is exercised too. */
function details() {
  const route = {
    rank: 1,
    sampleCount: 54_814,
    pickRate: 0.68,
    winRate: 0.65,
    purchaseOrder: [item(3032), item(123_430), item(6333)],
    starters: [
      {
        rank: 2,
        sampleCount: 9_896,
        pickRate: 0.18,
        winRate: 0.66,
        items: [item(1042), item(3006)],
      },
      { rank: 1, sampleCount: 27_712, pickRate: 0.5, winRate: 0.66, items: [item(1038)] },
    ],
    boots: [{ rank: 1, sampleCount: 25_804, pickRate: 0.47, winRate: 0.67, item: item(3006) }],
    laterItems: [{ rank: 1, sampleCount: 6_886, pickRate: 0.12, winRate: 0.63, item: item(3065) }],
  };
  return readChampionDetails({
    builds: {
      filtered: {
        archetypes: [
          {
            key: "bruiser",
            rank: 2,
            sampleCount: 100,
            pickRate: 0.2,
            winRate: 0.55,
            profiles: [{ rank: 1, routes: [route] }],
          },
          {
            key: "crit",
            rank: 1,
            sampleCount: 858_201,
            pickRate: 0.6,
            winRate: 0.6,
            profiles: [{ rank: 1, routes: [route] }],
          },
        ],
      },
    },
    items: {
      filtered: {
        all: [
          // Already in the core, so it must not reappear as situational.
          { rank: 1, id: 3032, sampleCount: 50_000, pickRate: 0.6, winRate: 0.7 },
          { rank: 2, id: 3078, sampleCount: 40_000, pickRate: 0.3, winRate: 0.66 },
          // Below the sample floor: excluded.
          { rank: 3, id: 9999, sampleCount: 10, pickRate: 0.01, winRate: 0.99 },
        ],
      },
    },
  });
}

describe("item sets", () => {
  it("are one per archetype, best first", () => {
    expect(archetypes(details()).map((b) => b.key)).toEqual(["crit", "bruiser"]);
  });

  it("take their blocks from the best route", () => {
    const crit = archetypes(details())[0];

    expect(crit?.core, "core follows the purchase order").toEqual([3032, 123_430, 6333]);
    expect(crit?.boots.map((b) => b.id)).toEqual([3006]);
    // Sorted by pick rate, so the most common starter leads.
    expect(crit?.starters[0]?.items).toEqual([1038]);
    expect(crit?.starters[1]?.items).toEqual([1042, 3006]);
  });

  it("never repeat an item already in the set as situational", () => {
    const ids = archetypes(details())[0]?.situational.map((i) => i.id) ?? [];

    expect(ids, "the route's later items come first").toContain(3065);
    expect(ids, "strong items not already placed are added").toContain(3078);
    expect(ids, "3032 is already in the core").not.toContain(3032);
    expect(ids, `below the ${MIN_SAMPLES} game floor`).not.toContain(9999);
  });
});
