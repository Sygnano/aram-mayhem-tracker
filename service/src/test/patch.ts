/**
 * A small but real-shaped patch on a fake upstream, shared by the route and crawler tests: three
 * ranked champions, one of them missing upstream, two Mayhem rarities and the 16.19 anvil fixtures.
 */

import { readFileSync } from "node:fs";
import { expect } from "vitest";
import { crawlFrom, fakeUpstream, testService } from "./helpers.ts";

export const DATA = "/data/16.19-test/stats/all";
export const CDRAGON = "/16.19/plugins/rcp-be-lol-game-data/global/default/v1";
const fixture = (name: string) =>
  readFileSync(new URL(`../../fixtures/anvils/${name}`, import.meta.url), "utf8");

export const row = (id: number, winRate: number, sampleCount: number, extra = {}) => ({
  id,
  rank: 1,
  tier: "S",
  sampleCount,
  pickRate: 0.1,
  winRate,
  augmentWinRate: 0.52,
  availableStages: [1, 2, 3, 4],
  ...extra,
});

export const champion = (
  id: number,
  winRate: number,
  damage: [number, number, number],
  extra = {},
) => ({
  champion: {
    id,
    tier: "S",
    rank: 1,
    stats: {
      sampleCount: 3_138_960,
      winRate,
      pickRate: 0.1,
      physicalDamageToChampions: damage[0],
      magicDamageToChampions: damage[1],
      trueDamageToChampions: damage[2],
    },
  },
  ...extra,
});

export const VERSIONS = {
  latest: "16.19",
  versions: [
    { version: "16.19", dataPath: "data/16.19-test", dataDate: "2026-09-27", allMatches: 4242 },
    { version: "16.18", dataPath: "data/16.18-old" },
  ],
};

/**
 * A small but real-shaped patch: three ranked champions, one of them missing upstream. Crawled into
 * the service before it is handed over, unless `withVersion` is false.
 */
export async function service({ adminToken = null as string | null, withVersion = true } = {}) {
  const upstream = await fakeUpstream({
    "/data/versions.json": { json: VERSIONS },
    [`${DATA}/champion-rankings.json`]: {
      json: {
        rows: [
          { id: 103, rank: 2, tier: "", sampleCount: 9, winRate: 0.51, pickRate: 0.02 },
          { id: 157, rank: 1, tier: "S", sampleCount: 9, winRate: 0.5773, pickRate: 0.1 },
          { id: 432, rank: 3, tier: "D", sampleCount: 9, winRate: 0.402, pickRate: 0.01 },
        ],
      },
    },
    [`${DATA}/augment-rankings.json`]: {
      json: { all: [row(1134, 0.5996, 2_762_899), row(1058, 0.55, 2_000_000)], stages: {} },
    },
    [`${DATA}/champion-details/157.json`]: {
      json: champion(157, 0.5773, [35112, 5079, 2097], {
        augments: {
          all: [row(1058, 0.6661, 320_503), row(2031, 0.61, 5_000)],
          stages: { "1": [row(1058, 0.9, 60)] },
        },
        builds: {
          filtered: {
            archetypes: [
              {
                key: "crit",
                rank: 1,
                profiles: [{ rank: 1, itemSet: [{ id: 3031 }], routes: [] }],
              },
            ],
          },
        },
      }),
    },
    [`${DATA}/champion-details/103.json`]: { json: champion(103, 0.51, [0, 10_000, 0]) },
    [`${CDRAGON}/cherry-augments.json`]: {
      json: [
        {
          id: 1058,
          augmentNameId: "MysticPunch",
          nameTRA: "Mystic Punch",
          rarity: "kGold",
          augmentSmallIconPath: "/lol-game-data/assets/x/mp.png",
        },
        {
          id: 1134,
          augmentNameId: "DrawYourSword",
          nameTRA: "Draw Your Sword",
          rarity: "kGold",
          augmentSmallIconPath: "",
        },
        {
          id: 2031,
          augmentNameId: "SilverOne",
          nameTRA: "Silver One",
          rarity: "kSilver",
          augmentSmallIconPath: "",
        },
        {
          id: 9001,
          augmentNameId: "NewThing",
          nameTRA: "New Thing",
          rarity: "kGold",
          augmentSmallIconPath: "",
        },
      ],
    },
    [`${CDRAGON}/augment-lists.json`]: {
      json: [
        {
          modeName: "KIWI",
          augmentList: ["A/MysticPunch", "A/DrawYourSword", "A/NewThing", "A/SilverOne"],
        },
      ],
    },
    "/16.19/game/data/maps/shipping/map12/map12.bin.json": {
      text: fixture("map12.anvils.16.19.json"),
    },
    "/16.19/game/en_us/data/menu/en_us/lol.stringtable.json": {
      text: fixture("stringtable.en_us.16.19.json"),
    },
    "/16.19/game/fr_fr/data/menu/en_us/lol.stringtable.json": {
      text: fixture("stringtable.fr_fr.16.19.json"),
    },
  });
  const svc = await testService({ upstreamBase: upstream.base, adminToken });
  if (withVersion) {
    const crawled = await crawlFrom(svc.crawl, upstream.base);
    expect(crawled.complete).toBe(true);
  }
  return { ...svc, upstream };
}
