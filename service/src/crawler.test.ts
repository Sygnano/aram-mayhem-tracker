/** The crawler end to end: a fake aramkit, the real service over HTTP, and the crawl routes' guard. */

import { describe, expect, it } from "vitest";
import type { CrawlerConfig } from "./config.ts";
import { runCrawl } from "./crawler.ts";
import { silentLog } from "./test/helpers.ts";
import { DATA, service } from "./test/patch.ts";

/** The service listening on a random port, so the crawler reaches it the way it does on Railway. */
async function listening({ adminToken = "s3cret" as string | null } = {}) {
  const svc = await service({ adminToken, withVersion: false });
  const serviceUrl = await svc.app.listen({ host: "127.0.0.1", port: 0 });
  const config: CrawlerConfig = {
    serviceUrl,
    adminToken: "s3cret",
    aramkitBase: svc.upstream.base,
    userAgent: "test",
    intervalMs: 5_000,
    maxFailures: 3,
  };
  const waits: number[] = [];
  const wait = async (ms: number) => {
    waits.push(ms);
  };
  return { ...svc, config, waits, wait };
}

describe("the crawler", () => {
  it("crawls a version one document at a time, and the next run only reads versions.json", async () => {
    const { app, config, upstream, wait, waits } = await listening();

    const outcome = await runCrawl({ config, log: silentLog, wait });
    // versions.json, the two rankings, and three champions (432 answers 404 and is recorded).
    expect(outcome).toEqual({ dataPath: "data/16.19-test", fetched: 6, complete: true });
    expect(waits, "five seconds before every request after versions.json").toEqual(
      Array(5).fill(5_000),
    );
    expect((await app.inject("/v1/champions")).statusCode).toBe(200);

    const again = await runCrawl({ config, log: silentLog, wait });
    expect(again).toEqual({ dataPath: "data/16.19-test", fetched: 1, complete: true });
    expect(upstream.hits(`${DATA}/champion-details/157.json`)).toBe(1);
  });

  it("gives up after failures in a row, and the next run resumes where it stopped", async () => {
    const { app, config, upstream, wait } = await listening();
    const path = `${DATA}/champion-details/157.json`;
    const kept = upstream.answers[path];
    upstream.answers[path] = { status: 503 };

    await expect(runCrawl({ config, log: silentLog, wait })).rejects.toThrow(
      /giving up after 3 failures/,
    );
    expect((await app.inject("/v1/champions")).statusCode, "not served half crawled").toBe(503);

    if (kept !== undefined) {
      upstream.answers[path] = kept;
    }
    const resumed = await runCrawl({ config, log: silentLog, wait });
    expect(resumed.complete).toBe(true);
    // versions.json, the champion that failed and the one after it (432, a 404): nothing already sent.
    expect(resumed.fetched).toBe(3);
    expect(upstream.hits(`${DATA}/champion-details/103.json`)).toBe(1);
  });
});

describe("the crawl routes", () => {
  it("need the admin token, and are off on a deploy without one", async () => {
    const { app } = await service({ adminToken: "s3cret" });
    expect((await app.inject("/admin/crawl")).statusCode).toBe(401);
    const wrong = await app.inject({
      url: "/admin/crawl",
      headers: { authorization: "Bearer nope" },
    });
    expect(wrong.statusCode).toBe(401);
    const right = await app.inject({
      url: "/admin/crawl",
      headers: { authorization: "Bearer s3cret" },
    });
    expect(right.json()).toMatchObject({
      dataPath: "data/16.19-test",
      complete: true,
      missing: [],
    });

    const { app: locked } = await service();
    expect(
      (await locked.inject({ url: "/admin/crawl", headers: { authorization: "Bearer s3cret" } }))
        .statusCode,
    ).toBe(403);
  });

  it("refuse a document that is not an octet-stream body", async () => {
    const { app } = await service({ adminToken: "s3cret" });
    const response = await app.inject({
      method: "PUT",
      url: "/admin/crawl/doc?dataPath=data/16.19-test&kind=champion-details&key=157",
      headers: { authorization: "Bearer s3cret", "content-type": "application/json" },
      payload: { champion: { id: 157 } },
    });
    expect(response.statusCode).toBe(400);
  });
});
