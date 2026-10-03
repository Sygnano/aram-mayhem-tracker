/** `/v1/dataset`: the whole dataset, gzipped once and served as stored, with an ETag. */

import { gunzipSync } from "node:zlib";
import { describe, expect, it } from "vitest";
import { service } from "../test/patch.ts";

const gzip = { "accept-encoding": "gzip" };

describe("/v1/dataset", () => {
  it("holds every champion's bundle as /v1/champion sends it, the table and every anvil group", async () => {
    const { app } = await service({ adminToken: "s3cret" });
    const tiers = { silver: [["ARAM_StatAnvil_AR"]] };
    await app.inject({
      method: "PUT",
      url: "/v1/anvil-rankings",
      payload: { version: 1, groups: [{ name: "Swordsmen", champions: [157], tiers }] },
      headers: { authorization: "Bearer s3cret" },
    });

    const response = await app.inject({ url: "/v1/dataset", headers: gzip });
    expect(response.statusCode).toBe(200);
    expect(response.headers["content-encoding"]).toBe("gzip");
    expect(response.headers["content-type"]).toBe("application/json; charset=utf-8");
    const body = JSON.parse(gunzipSync(response.rawPayload).toString("utf8"));

    expect(body).toMatchObject({ patch: "16.19", dataDate: "2026-09-27" });
    // 432 is ranked but upstream has no details for it: left out, not an error.
    expect(Object.keys(body.champions)).toEqual(["103", "157"]);
    const single = (await app.inject("/v1/champion?champion=157")).json();
    expect(body.champions["157"]).toEqual({
      champion: single.champion,
      pools: single.pools,
      archetypes: single.archetypes,
    });
    expect(body.championTable).toEqual({
      poolSize: 3,
      champions: (await app.inject("/v1/champions")).json().champions,
    });
    expect(body.anvilRankings.groups.map((g: { name: string }) => g.name)).toEqual(["Swordsmen"]);
  });

  it("answers 304 to the copy a client holds, until the data or the rankings change", async () => {
    const { app } = await service({ adminToken: "s3cret" });
    const first = await app.inject({ url: "/v1/dataset", headers: gzip });
    const etag = first.headers.etag as string;
    expect(etag).toMatch(/^"[0-9a-f]{32}"$/);

    const again = await app.inject({
      url: "/v1/dataset",
      headers: { ...gzip, "if-none-match": etag },
    });
    expect(again.statusCode).toBe(304);
    expect(again.rawPayload.length).toBe(0);

    await app.inject({
      method: "PUT",
      url: "/v1/anvil-rankings",
      payload: { version: 1, groups: [] },
      headers: { authorization: "Bearer s3cret" },
    });
    const changed = await app.inject({
      url: "/v1/dataset",
      headers: { ...gzip, "if-none-match": etag },
    });
    expect(changed.statusCode).toBe(200);
    expect(changed.headers.etag).not.toBe(etag);
  });

  it("is built once however many ask at the same time, and unpacked for a client without gzip", async () => {
    const { app, datasets } = await service();
    const answers = await Promise.all(
      Array.from({ length: 5 }, () => app.inject({ url: "/v1/dataset", headers: gzip })),
    );
    const built = await datasets.get();
    for (const answer of answers) {
      expect(answer.rawPayload.equals(built.gzip)).toBe(true);
    }

    const plain = await app.inject({
      url: "/v1/dataset",
      headers: { "accept-encoding": "identity" },
    });
    expect(plain.headers["content-encoding"]).toBeUndefined();
    expect(plain.json().patch).toBe("16.19");
  });

  it("is a 503 before any version is crawled", async () => {
    const { app } = await service({ withVersion: false });
    expect((await app.inject({ url: "/v1/dataset", headers: gzip })).statusCode).toBe(503);
  });
});
