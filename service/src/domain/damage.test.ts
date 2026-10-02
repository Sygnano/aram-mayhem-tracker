import { describe, expect, it } from "vitest";
import { readChampionDetails } from "../upstream/shapes.ts";
import { damageProfile, teamDamage } from "./damage.ts";

function detailsWithDamage(id: number, physical: number, magic: number, trueDamage: number) {
  return readChampionDetails({
    champion: {
      id,
      stats: {
        sampleCount: 1000,
        physicalDamageToChampions: physical,
        magicDamageToChampions: magic,
        trueDamageToChampions: trueDamage,
      },
    },
  });
}

describe("damage profiles", () => {
  it("read the damage split as shares", () => {
    // Yasuo on 16.19, 2026-09-29: 35112 physical, 5079 magic, 2097 true.
    const p = damageProfile(detailsWithDamage(157, 35112, 5079, 2097));
    expect(p).not.toBeNull();
    expect(p?.physical).toBeCloseTo(0.8303, 4);
    expect(p?.magic).toBeCloseTo(0.1201, 4);
    expect(p?.trueDamage).toBeCloseTo(0.0496, 4);
    expect((p?.physical ?? 0) + (p?.magic ?? 0) + (p?.trueDamage ?? 0)).toBeCloseTo(1, 9);
    expect(p?.damagePerGame).toBe(42288);

    // A document with no damage in it is a champion upstream has no games for.
    expect(damageProfile(readChampionDetails({}))).toBeNull();
  });

  it("are read from the real field names", () => {
    const details = readChampionDetails(
      JSON.parse(`{ "champion": { "id": 157, "stats": { "sampleCount": 3138960,
        "damageToChampions": 42289, "physicalDamageToChampions": 35112,
        "magicDamageToChampions": 5079, "trueDamageToChampions": 2097 } } }`),
    );
    const p = damageProfile(details);
    expect([p?.id, p?.sampleCount, p?.damagePerGame]).toEqual([157, 3138960, 42288]);
  });

  it("weight a team by how much each champion deals", () => {
    // A carry dealing 30k, all physical, beside a support dealing 10k, all magic: the team is three
    // quarters physical, not half.
    const carry = damageProfile(detailsWithDamage(1, 30_000, 0, 0));
    const support = damageProfile(detailsWithDamage(2, 0, 10_000, 0));
    if (carry === null || support === null) {
      throw new Error("both champions deal damage");
    }
    const team = teamDamage([carry, support]);
    expect(team?.physical).toBeCloseTo(0.75, 9);
    expect(team?.magic).toBeCloseTo(0.25, 9);
    expect(teamDamage([])).toBeNull();
  });
});
