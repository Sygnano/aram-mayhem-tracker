import { describe, expect, it } from "vitest";
import { authorize } from "../routes/anvils.ts";
import type { AnvilCatalogue, Tier } from "./anvils.ts";
import { DOC_VERSION, type Group, normaliseDoc, type RankingDoc, validate } from "./rankings.ts";

function shard(id: string, tier: Tier) {
  return { id, tier, kind: "MinAR", name: id, values: [1], iconUrl: "" };
}

const catalogue: AnvilCatalogue = {
  patch: "16.19",
  locale: "en_us",
  shards: [
    shard("ARAM_StatAnvil_AR", "silver"),
    shard("ARAM_StatAnvil_MR", "silver"),
    shard("ARAM_GoldStatAnvil_AR", "gold"),
    shard("ARAM_StatAnvil_HS", "prismatic"),
  ],
};

function group(name: string, champions: number[], silver: string[][]): Group {
  return { name, champions, tiers: { silver, gold: [], prismatic: [] } };
}

const doc = (groups: Group[]): RankingDoc => ({ version: DOC_VERSION, groups });

describe("validating the rankings", () => {
  it("accepts a tie as a shared bucket", () => {
    const d = doc([group("Tanks", [1, 2], [["ARAM_StatAnvil_AR", "ARAM_StatAnvil_MR"]])]);
    expect(validate(d, catalogue)).toEqual([]);
  });

  it("refuses a champion in two groups", () => {
    const problems = validate(doc([group("A", [7], []), group("B", [7], [])]), catalogue);
    expect(problems).toHaveLength(1);
    expect(problems[0]).toContain("champion 7");
  });

  it("refuses a shard in the wrong tier, or an unknown one", () => {
    const d = doc([group("A", [], [["ARAM_GoldStatAnvil_AR"], ["ARAM_StatAnvil_Nope"]])]);
    expect(validate(d, catalogue)).toHaveLength(2);
  });

  it("refuses duplicates, empty buckets and repeated or empty names", () => {
    const d = doc([
      group("A", [], [["ARAM_StatAnvil_AR"], [], ["ARAM_StatAnvil_AR"]]),
      group(" a ", [], []),
      group("  ", [], []),
    ]);
    expect(validate(d, catalogue)).toHaveLength(4);
  });
});

describe("the rankings document", () => {
  it("round-trips through JSON with its save time", () => {
    const d = doc([group("A", [1], [["ARAM_StatAnvil_AR"]])]);
    const json = JSON.parse(JSON.stringify({ savedAt: 5, ...d }));
    expect(json.savedAt).toBe(5);
    expect(json.groups[0].tiers.silver[0][0]).toBe("ARAM_StatAnvil_AR");
    // The editor sends back what it loaded, savedAt included; it must still read the same.
    expect(normaliseDoc(json)).toEqual(d);
  });

  it("fills in what a client left out", () => {
    expect(normaliseDoc({ version: 1, groups: [{ name: "A", tiers: { gold: [["x"]] } }] })).toEqual(
      doc([{ name: "A", champions: [], tiers: { silver: [], gold: [["x"]], prismatic: [] } }]),
    );
  });
});

describe("saving", () => {
  it("is locked without a token and checks the token with one", () => {
    expect(() => authorize(null, undefined)).toThrow(/forbidden/);
    expect(() => authorize("", undefined)).toThrow(/forbidden/);
    expect(() => authorize("s3cret", undefined)).toThrow(/unauthorized/);
    expect(() => authorize("s3cret", "Bearer s3cre")).toThrow(/unauthorized/);
    expect(() => authorize("s3cret", "bearer s3cret")).toThrow(/unauthorized/);
    expect(() => authorize("s3cret", "Bearer s3cret")).not.toThrow();
  });
});
