import { describe, expect, it } from "vitest";
import { iconUrl, normaliseRarity, readAugmentRows } from "./shapes.ts";

describe("CommunityDragon metadata", () => {
  it("normalises rarities to what the vision crate produces", () => {
    expect(normaliseRarity("kPrismatic")).toBe("prismatic");
    expect(normaliseRarity("kGold")).toBe("gold");
    expect(normaliseRarity("kSilver")).toBe("silver");
  });

  it("turns icon paths into URLs", () => {
    expect(
      iconUrl(
        "https://raw.communitydragon.org",
        "16.19",
        "/lol-game-data/assets/ASSETS/UX/Cherry/Augments/Icons/backtobasics.png",
      ),
    ).toBe(
      "https://raw.communitydragon.org/16.19/plugins/rcp-be-lol-game-data/global/default/assets/ux/cherry/augments/icons/backtobasics.png",
    );
  });
});

describe("the lenient readers", () => {
  it("default what is missing or mistyped instead of failing the document", () => {
    const rows = readAugmentRows({
      all: [{ id: 1058, winRate: "high", sampleCount: 12.5 }, "not a row"],
      stages: { "1": [{ id: 1058 }], "2": null },
    });
    expect(rows.all[0]).toMatchObject({ id: 1058, winRate: 0, sampleCount: 0, tier: "" });
    expect(rows.all[1]?.id).toBe(0);
    expect(rows.stages["1"]).toHaveLength(1);
    expect(rows.stages["2"]).toEqual([]);
  });
});
