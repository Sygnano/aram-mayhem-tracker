/** Every data endpoint, end to end, over a fake aramkit and CommunityDragon. */

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { fakeUpstream, insertVersion, silentLog, testService } from "../test/helpers.ts";
import { refreshVersions } from "../upstream/versions.ts";

const DATA = "/data/16.19-test/stats/all";
const CDRAGON = "/16.19/plugins/rcp-be-lol-game-data/global/default/v1";
const fixture = (name: string) =>
  readFileSync(new URL(`../../fixtures/anvils/${name}`, import.meta.url), "utf8");

const row = (id: number, winRate: number, sampleCount: number, extra = {}) => ({
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

const champion = (id: number, winRate: number, damage: [number, number, number], extra = {}) => ({
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

/** A small but real-shaped patch: three ranked champions, one of them missing upstream. */
async function service({ adminToken = null as string | null, withVersion = true } = {}) {
  const upstream = await fakeUpstream({
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
    insertVersion(svc.db);
  }
  return { ...svc, upstream };
}

describe("/v1/pool", () => {
  it("ranks one rarity's pool, champion data first, unseen augments last", async () => {
    const { app } = await service();
    const response = await app.inject("/v1/pool?champion=157&rarity=Gold&stage=1");
    expect(response.statusCode).toBe(200);
    const body = response.json();

    expect(body).toMatchObject({
      patch: "16.19",
      dataDate: "2026-09-27",
      champion: { id: 157, winRate: 0.5773, sampleCount: 3_138_960, tier: "S" },
      rarity: "gold",
      stage: 1,
    });
    expect(body.augments.map((a: { id: number }) => a.id)).toEqual([1058, 1134, 9001]);

    const [punch, sword, unseen] = body.augments;
    // The stage row has 60 games, under the floor, so the champion row decides.
    expect(punch).toMatchObject({
      nameId: "MysticPunch",
      name: "Mystic Punch",
      rarity: "gold",
      iconUrl: expect.stringMatching(
        /\/16\.19\/plugins\/rcp-be-lol-game-data\/global\/default\/x\/mp\.png$/,
      ),
      winRate: 0.6661,
      source: "champion",
      confidence: "high",
      deltaBasis: "champion",
      lowSample: false,
      availableStages: [1, 2, 3, 4],
    });
    expect(punch.deltaPp).toBeCloseTo(8.88, 2);
    expect(punch.byStage).toEqual([
      {
        stage: 1,
        level: 1,
        winRate: 0.9,
        deltaPp: expect.closeTo(32.27, 2),
        pickRate: 0.1,
        sampleCount: 60,
        lowSample: true,
      },
    ]);
    expect(sword).toMatchObject({ source: "global", confidence: "fallback", deltaBasis: "global" });
    expect(unseen).toMatchObject({
      name: "New Thing",
      winRate: null,
      deltaPp: null,
      source: "none",
      tier: null,
      rank: null,
      byStage: [],
    });
  });

  it("refuses an unknown rarity", async () => {
    const { app } = await service();
    const response = await app.inject("/v1/pool?champion=157&rarity=bronze");
    expect(response.statusCode).toBe(400);
    expect(response.json().error).toBe(
      'bad request: rarity must be silver, gold or prismatic, not "bronze"',
    );
  });
});

describe("/v1/offer", () => {
  it("ranks the cards it is given", async () => {
    const { app } = await service();
    const body = (await app.inject("/v1/offer?champion=157&augments=1134,1058")).json();
    expect(body.stage).toBeNull();
    expect(body.ranking).toEqual([1058, 1134]);
    expect(body.augments).toHaveLength(2);
  });

  it("refuses a champion aramkit does not rank, without asking upstream", async () => {
    const { app, upstream } = await service();
    const response = await app.inject("/v1/offer?champion=999&augments=1058");
    expect(response.statusCode).toBe(404);
    expect(response.json()).toEqual({
      error: "not found: no champion with id 999 in this patch",
      retryAfter: null,
    });
    expect(upstream.hits(`${DATA}/champion-details/999.json`)).toBe(0);
  });

  it("is a 404 for a ranked champion upstream has no document for", async () => {
    const { app } = await service();
    const response = await app.inject("/v1/offer?champion=432&augments=1058");
    expect(response.statusCode).toBe(404);
    expect(response.json().error).toBe("not found: upstream has no data for this champion");
  });

  it("is a 503 before the first versions.json read, and a 400 on a malformed query", async () => {
    const { app } = await service({ withVersion: false });
    expect((await app.inject("/v1/offer?champion=157&augments=1058")).statusCode).toBe(503);
    const bad = await app.inject("/v1/offer?champion=yasuo&augments=1058");
    expect(bad.statusCode).toBe(400);
    expect(bad.json().error).toMatch(/^bad request: /);
  });
});

describe("/v1/champions", () => {
  it("lists every ranked champion, best first, with what a rank is out of", async () => {
    const body = (await (await service()).app.inject("/v1/champions")).json();
    expect(body.poolSize).toBe(3);
    expect(body.champions).toEqual([
      { id: 157, rank: 1, tier: "S", winRate: 0.5773, pickRate: 0.1, sampleCount: 9 },
      { id: 103, rank: 2, tier: null, winRate: 0.51, pickRate: 0.02, sampleCount: 9 },
      { id: 432, rank: 3, tier: "D", winRate: 0.402, pickRate: 0.01, sampleCount: 9 },
    ]);
  });
});

describe("/v1/build", () => {
  it("answers one item set per archetype", async () => {
    const body = (await (await service()).app.inject("/v1/build?champion=157")).json();
    expect(body).toMatchObject({ patch: "16.19", championId: 157 });
    expect(body.archetypes).toEqual([
      {
        key: "crit",
        rank: 1,
        winRate: 0,
        pickRate: 0,
        sampleCount: 0,
        starters: [],
        boots: [],
        core: [3031],
        situational: [],
      },
    ]);
  });
});

describe("/v1/champion", () => {
  it("answers in one response what the pools, the build and the rankings answer separately", async () => {
    const { app, upstream } = await service({ adminToken: "s3cret" });
    const tiers = { silver: [["ARAM_StatAnvil_AR"]] };
    const saved = await app.inject({
      method: "PUT",
      url: "/v1/anvil-rankings",
      payload: {
        version: 1,
        groups: [
          { name: "Tanks", champions: [1, 2], tiers },
          { name: "Swordsmen", champions: [157], tiers },
        ],
      },
      headers: { authorization: "Bearer s3cret" },
    });
    expect(saved.statusCode).toBe(200);

    const response = await app.inject("/v1/champion?champion=157");
    expect(response.statusCode).toBe(200);
    const body = response.json();
    expect(body).toMatchObject({ patch: "16.19", dataDate: "2026-09-27" });
    expect(body.champion.id).toBe(157);

    // Twelve pools, rarity then stage, each exactly what /v1/pool answers.
    expect(
      body.pools.map((p: { rarity: string; stage: number }) => `${p.rarity}${p.stage}`),
    ).toEqual([
      "silver1",
      "silver2",
      "silver3",
      "silver4",
      "gold1",
      "gold2",
      "gold3",
      "gold4",
      "prismatic1",
      "prismatic2",
      "prismatic3",
      "prismatic4",
    ]);
    for (const pool of body.pools) {
      const single = (
        await app.inject(`/v1/pool?champion=157&rarity=${pool.rarity}&stage=${pool.stage}`)
      ).json();
      expect(pool.augments).toEqual(single.augments);
    }
    expect(body.archetypes).toEqual((await app.inject("/v1/build?champion=157")).json().archetypes);

    // Only this champion's group, with when the rankings were saved.
    expect(body.anvilRankings.savedAt).toBe(saved.json().savedAt);
    expect(body.anvilRankings.groups.map((g: { name: string }) => g.name)).toEqual(["Swordsmen"]);

    // One champion document fetched upstream, however many sections it fed.
    expect(upstream.hits(`${DATA}/champion-details/157.json`)).toBe(1);
  });

  it("has no anvil group for a champion in none, and refuses a champion aramkit does not rank", async () => {
    const { app } = await service();
    const body = (await app.inject("/v1/champion?champion=157")).json();
    expect(body.anvilRankings).toEqual({ savedAt: null, version: 1, groups: [] });

    const unknown = await app.inject("/v1/champion?champion=999999");
    expect(unknown.statusCode).toBe(404);
    expect((await app.inject("/v1/champion?champion=yasuo")).statusCode).toBe(400);
  });
});

describe("/v1/damage", () => {
  it("splits each champion and the team, and lists the ones it cannot", async () => {
    const { app, upstream } = await service();
    const body = (await app.inject("/v1/damage?champions=157,432,103,999,157")).json();

    expect(body.champions).toEqual([
      {
        id: 157,
        physical: 0.8303,
        magic: 0.1201,
        trueDamage: 0.0496,
        damagePerGame: 42288,
        sampleCount: 3_138_960,
      },
      {
        id: 103,
        physical: 0,
        magic: 1,
        trueDamage: 0,
        damagePerGame: 10000,
        sampleCount: 3_138_960,
      },
    ]);
    // 432 is ranked but upstream has no document; 999 is not ranked and is never fetched.
    expect(body.missing).toEqual([432, 999]);
    expect(upstream.hits(`${DATA}/champion-details/999.json`)).toBe(0);
    expect(body.team.physical).toBeCloseTo(35112 / 52288, 4);
    expect(body.team.physical + body.team.magic + body.team.trueDamage).toBeCloseTo(1, 3);
  });

  it("refuses too many champions", async () => {
    const response = await (await service()).app.inject(
      "/v1/damage?champions=1,2,3,4,5,6,7,8,9,10,11",
    );
    expect(response.statusCode).toBe(400);
  });
});

describe("/v1/anvils", () => {
  it("builds the catalogue once per patch and locale", async () => {
    const { app, upstream } = await service();
    const table = "/16.19/game/fr_fr/data/menu/en_us/lol.stringtable.json";

    const [first, second] = await Promise.all([
      app.inject("/v1/anvils?locale=FR_FR"),
      app.inject("/v1/anvils?locale=fr_fr"),
    ]);
    expect(first.statusCode).toBe(200);
    expect(first.json()).toEqual(second.json());
    expect(first.json()).toMatchObject({ patch: "16.19", locale: "fr_fr" });
    expect(first.json().shards).toHaveLength(34);
    expect(upstream.hits(table), "concurrent requests share one build").toBe(1);

    await app.inject("/v1/anvils?locale=fr_fr");
    expect(upstream.hits(table), "the derived catalogue is cached").toBe(1);
  });

  it("refuses a locale CommunityDragon does not publish", async () => {
    const response = await (await service()).app.inject("/v1/anvils?locale=xx_xx");
    expect(response.statusCode).toBe(400);
    expect(response.json().error).toMatch(/unknown locale "xx_xx"/);
  });
});

describe("/v1/anvil-rankings", () => {
  const valid = {
    version: 1,
    savedAt: 12,
    groups: [
      {
        name: " Tanks ",
        champions: [1, 2],
        tiers: { silver: [["ARAM_StatAnvil_AR", "ARAM_StatAnvil_MR"]] },
      },
    ],
  };
  const put = (app: Awaited<ReturnType<typeof service>>["app"], body: object, token?: string) =>
    app.inject({
      method: "PUT",
      url: "/v1/anvil-rankings",
      payload: body,
      headers: token ? { authorization: `Bearer ${token}` } : {},
    });

  it("is an empty document before the first save", async () => {
    const body = (await (await service()).app.inject("/v1/anvil-rankings")).json();
    expect(body).toEqual({ savedAt: null, version: 1, groups: [] });
  });

  it("refuses to save without ADMIN_TOKEN, or with the wrong token", async () => {
    expect((await put((await service()).app, valid, "x")).statusCode).toBe(403);
    expect((await put((await service({ adminToken: "s3cret" })).app, valid, "x")).statusCode).toBe(
      401,
    );
  });

  it("saves a valid document and serves it back, trimmed and normalised", async () => {
    const { app } = await service({ adminToken: "s3cret" });
    const saved = await put(app, valid, "s3cret");
    expect(saved.statusCode).toBe(200);

    const current = (await app.inject("/v1/anvil-rankings")).json();
    expect(current).toEqual(saved.json());
    expect(current.savedAt).toBeGreaterThan(12);
    expect(current.groups).toEqual([
      {
        name: "Tanks",
        champions: [1, 2],
        tiers: { silver: [["ARAM_StatAnvil_AR", "ARAM_StatAnvil_MR"]], gold: [], prismatic: [] },
      },
    ]);
  });

  it("refuses an invalid document and keeps the previous save", async () => {
    const { app } = await service({ adminToken: "s3cret" });
    await put(app, valid, "s3cret");

    const refused = await put(
      app,
      { version: 1, groups: [{ name: "A", tiers: { gold: [["ARAM_StatAnvil_AR"]] } }] },
      "s3cret",
    );
    expect(refused.statusCode).toBe(400);
    expect(refused.json().error).toBe(
      'bad request: not saved, the previous save is unchanged: group "A", gold: ARAM_StatAnvil_AR is not a gold shard this patch',
    );
    expect((await app.inject("/v1/anvil-rankings")).json().groups[0].name).toBe("Tanks");
  });
});

describe("/admin", () => {
  it("serves the editor, unframeable and outside the rate limit", async () => {
    const response = await (await service()).app.inject("/admin");
    expect(response.statusCode).toBe(200);
    expect(response.headers["content-type"]).toBe("text/html; charset=utf-8");
    expect(response.headers["x-frame-options"]).toBe("DENY");
    expect(response.headers["content-security-policy"]).toBe(
      "frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
    );
    expect(response.headers["referrer-policy"]).toBe("no-referrer");
    expect(response.body).toContain("<html");
  });
});

describe("versions.json", () => {
  it("sets the patch every endpoint answers for", async () => {
    const { app, upstream, store, client } = await service({ withVersion: false });
    upstream.answers["/data/versions.json"] = {
      json: {
        latest: "16.19",
        versions: [
          { version: "16.19", dataPath: "data/16.19-test", dataDate: "2026-09-27", allMatches: 7 },
          { version: "16.18", dataPath: "data/16.18-old" },
        ],
      },
    };
    const latest = await refreshVersions({
      client,
      store,
      aramkitBase: upstream.base,
      log: silentLog,
    });
    expect(latest).toBe("16.19");
    expect((await app.inject("/v1/patch")).json()).toMatchObject({
      patch: "16.19",
      dataPath: "data/16.19-test",
      allMatches: 7,
    });
  });
});
