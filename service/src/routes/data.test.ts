/** Every data endpoint, end to end, over a fake aramkit and CommunityDragon. */

import { describe, expect, it } from "vitest";
import { crawlFrom } from "../test/helpers.ts";
import { DATA, service, VERSIONS } from "../test/patch.ts";

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

describe("versions.json and the crawl", () => {
  it("serves a version only once every document is in, champions upstream lacks included", async () => {
    const { app, crawl, upstream } = await service({ withVersion: false });
    expect((await app.inject("/v1/patch")).statusCode).toBe(503);

    let status = await crawl.versions(Buffer.from(JSON.stringify(VERSIONS)));
    expect(status).toMatchObject({ dataPath: "data/16.19-test", complete: false, expected: null });
    // The two rankings first; the champions are only known once the rankings are in.
    expect(status.missing.map((m) => m.path)).toEqual([
      "data/16.19-test/stats/all/augment-rankings.json",
      "data/16.19-test/stats/all/champion-rankings.json",
    ]);

    status = await crawlFrom(crawl, upstream.base);
    expect(status).toMatchObject({ complete: true, done: 5, expected: 5, missing: [] });
    expect((await app.inject("/v1/patch")).json()).toMatchObject({
      patch: "16.19",
      dataPath: "data/16.19-test",
      allMatches: 4242,
    });
    // 432 is ranked but has no details upstream: recorded once, answered as upstream did.
    expect(upstream.hits(`${DATA}/champion-details/432.json`)).toBe(1);
    expect((await app.inject("/v1/champion?champion=432")).statusCode).toBe(404);
    expect(upstream.hits(`${DATA}/champion-details/432.json`), "never fetched on a request").toBe(
      1,
    );
  });

  it("refuses a document for another version, of an unknown kind, or that is not what it claims", async () => {
    const { crawl } = await service();
    const doc = Buffer.from(JSON.stringify({ champion: { id: 157 } }));
    await expect(crawl.ingest("data/16.18-old", "champion-details", "157", doc)).rejects.toThrow(
      /not the version being crawled/,
    );
    await expect(crawl.ingest("data/16.19-test", "secrets", "", doc)).rejects.toThrow(
      /unknown document kind/,
    );
    await expect(crawl.ingest("data/16.19-test", "champion-details", "103", doc)).rejects.toThrow(
      /is about champion 157/,
    );
    await expect(
      crawl.ingest("data/16.19-test", "champion-details", "157", Buffer.from("[1]")),
    ).rejects.toThrow(/not a JSON object/);
    await expect(
      crawl.ingest("data/16.19-test", "champion-rankings", "", Buffer.from('{"rows":[]}')),
    ).rejects.toThrow(/lists no champion/);
  });

  it("cannot complete a version whose rankings upstream does not have", async () => {
    const { crawl, upstream } = await service({ withVersion: false });
    delete upstream.answers[`${DATA}/augment-rankings.json`];
    const status = await crawlFrom(crawl, upstream.base);
    expect(status.complete).toBe(false);
    expect(status.blocked).toMatch(/no augment-rankings for data\/16.19-test/);
  });
});
