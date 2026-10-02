import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { parseAnvilDocuments } from "./anvil-worker.ts";
import {
  type AnvilCatalogue,
  catalogue,
  type Shard,
  shardsFromMap,
  stringsFromTable,
  type Tier,
} from "./anvils.ts";

const fixture = (name: string) =>
  readFileSync(new URL(`../../fixtures/anvils/${name}`, import.meta.url));
const MAP = fixture("map12.anvils.16.19.json");
const EN = fixture("stringtable.en_us.16.19.json");
const FR = fixture("stringtable.fr_fr.16.19.json");
const BASE = "https://raw.communitydragon.org";

function build(table: Buffer, locale: string): AnvilCatalogue {
  const raw = shardsFromMap(JSON.parse(MAP.toString("utf8")));
  const names = stringsFromTable(
    JSON.parse(table.toString("utf8")),
    new Set(raw.map((r) => r.nameKey)),
  );
  return catalogue("16.19", locale, BASE, raw, names);
}

function find(c: AnvilCatalogue, tier: Tier, id: string): Shard {
  const shard = c.shards.find((s) => s.tier === tier && s.id === id);
  if (!shard) {
    throw new Error(`${id} missing from ${tier}`);
  }
  return shard;
}

describe("the anvil catalogue", () => {
  it("has pools of thirteen, thirteen and eight", () => {
    const c = build(EN, "en_us");
    const count = (t: Tier) => c.shards.filter((s) => s.tier === t).length;
    expect([count("silver"), count("gold"), count("prismatic")]).toEqual([13, 13, 8]);
    expect(
      c.shards.some((s) => s.id.endsWith("_Random") || s.id === "ARAM_StatAnvil_Pristine"),
    ).toBe(false);
  });

  /** Every value visible on the seven reference screenshots (anvil_*.png in mayhem-vision). */
  it("prints the values the cards print", () => {
    const c = build(EN, "en_us");
    const cases: [Tier, string, number[]][] = [
      ["silver", "ARAM_StatAnvil_AR", [12]],
      ["silver", "ARAM_StatAnvil_HP", [110]],
      ["silver", "ARAM_StatAnvil_AP", [15]],
      ["gold", "ARAM_GoldStatAnvil_Hybrid2", [25, 25]],
      ["gold", "ARAM_GoldStatAnvil_CritChance", [25]],
      ["gold", "ARAM_GoldStatAnvil_MPen", [18]],
      ["gold", "ARAM_GoldStatAnvil_MR", [45]],
      ["prismatic", "ARAM_StatAnvil_HS", [25]],
      ["prismatic", "ARAM_StatAnvil_MS", [15, -10]],
      ["prismatic", "ARAM_StatAnvil_Tenacity", [30]],
      ["prismatic", "ARAM_PrisStatAnvil_MPen", [17.5]],
    ];
    for (const [tier, id, values] of cases) {
      expect(find(c, tier, id).values, id).toEqual(values);
    }
  });

  it("gives the same stat one kind across tiers, with different values", () => {
    const c = build(EN, "en_us");
    const silver = find(c, "silver", "ARAM_StatAnvil_MR");
    const gold = find(c, "gold", "ARAM_GoldStatAnvil_MR");
    expect(silver.kind).toBe(gold.kind);
    expect(silver.values).not.toEqual(gold.values);

    // The property the app's tier decision rests on: within a kind, no two tiers share a first
    // value, so the first number on a card settles the tier.
    for (const a of c.shards) {
      for (const b of c.shards) {
        if (a.kind === b.kind && a.tier !== b.tier) {
          expect(a.values[0], `${a.id} vs ${b.id}`).not.toBe(b.values[0]);
        }
      }
    }
  });

  it("names shards in the requested locale, without their markup", () => {
    const en = build(EN, "en_us");
    expect(find(en, "gold", "ARAM_GoldStatAnvil_Hybrid2").name).toBe("Might Shard");
    expect(find(en, "prismatic", "ARAM_PrisStatAnvil_Lethality").name).toBe(
      "Armor Penetration Shard",
    );

    const fr = build(FR, "fr_fr");
    expect(find(fr, "silver", "ARAM_StatAnvil_AR").name).toBe("Fragment d'armure");
    expect(find(fr, "prismatic", "ARAM_PrisStatAnvil_MPen").name).toBe(
      "Fragment de pénétration magique",
    );
  });

  it("keeps only the wanted strings", () => {
    const names = stringsFromTable(
      JSON.parse(EN.toString("utf8")),
      new Set(["cherry_statanvil_ar_name"]),
    );
    expect(names.size).toBe(1);
    expect(names.get("cherry_statanvil_ar_name")).toBe("Armor Shard");
  });

  it("points icons at the converted PNG", () => {
    expect(find(build(EN, "en_us"), "gold", "ARAM_GoldStatAnvil_MR").iconUrl).toBe(
      "https://raw.communitydragon.org/16.19/game/assets/ux/cherry/augments/statanvil/gold_mr.png",
    );
  });

  it("parses the same on the worker thread as on the main one", async () => {
    const { raw, names } = await parseAnvilDocuments(MAP, FR);
    expect(catalogue("16.19", "fr_fr", BASE, raw, names)).toEqual(build(FR, "fr_fr"));
  });

  it("reports a worker that fails to parse as an error", async () => {
    await expect(parseAnvilDocuments(Buffer.from("{ nope"), FR)).rejects.toThrow(
      /parsing the anvil documents/,
    );
  });
});
