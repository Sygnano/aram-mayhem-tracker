import { describe, expect, it } from "vitest";
import { nowUnix } from "../db/database.ts";
import { insertVersion, testService } from "../test/helpers.ts";

const app = (
  options: {
    requestsPerSecond?: number;
    requestsPerSecondPerIp?: number;
    burstPerIp?: number;
  } = {},
) => testService(options);

describe("/v1/health", () => {
  it("is ok on an empty store, which is the correct state on a fresh deploy", async () => {
    const { app: a } = await app();
    const response = await a.inject("/v1/health");

    expect(response.statusCode).toBe(200);
    const body = response.json();
    expect(body).toEqual({
      ok: true,
      patch: null,
      dataDate: null,
      ageSeconds: null,
      championsCached: 0,
      crawling: null,
      uptimeSeconds: body.uptimeSeconds,
    });
    expect(body.uptimeSeconds).toBeGreaterThanOrEqual(5);
    expect(response.headers["x-content-type-options"]).toBe("nosniff");
  });

  it("reports the patch, its age and the champions cached for it", async () => {
    const { app: a, db } = await app();
    insertVersion(db, { dataPath: "data/x", firstSeenAt: nowUnix() - 60 });
    db.prepare(
      `INSERT INTO upstream_docs (data_path, source, kind, key, body, fetched_at)
       VALUES ('data/x', 'aramkit', 'champion-details', '157', x'00', 0),
              ('data/x', 'aramkit', 'champion-details', '103', x'00', 0),
              ('data/x', 'aramkit', 'augment-rankings', '', x'00', 0)`,
    ).run();

    const body = (await a.inject("/v1/health")).json();
    expect(body).toMatchObject({
      ok: true,
      patch: "16.19",
      dataDate: "2026-09-27",
      championsCached: 2,
    });
    expect(body.ageSeconds).toBeGreaterThanOrEqual(60);
  });

  it("is never rate limited, so Railway's check cannot fail on a busy service", async () => {
    const { app: a } = await app({ requestsPerSecond: 1 });
    for (let i = 0; i < 10; i++) {
      expect((await a.inject("/v1/health")).statusCode).toBe(200);
    }
  });
});

describe("/v1/patch", () => {
  it("is a 503 with Retry-After until the first crawl completes", async () => {
    const { app: a } = await app();
    const response = await a.inject("/v1/patch");

    expect(response.statusCode).toBe(503);
    expect(response.headers["retry-after"]).toBe("30");
    expect(response.json()).toEqual({
      error: "data not available yet: no data version has been crawled yet",
      retryAfter: 30,
    });
  });

  it("answers the latest patch", async () => {
    const { app: a, db } = await app();
    insertVersion(db, { dataPath: "data/x", firstSeenAt: 1234 });

    expect((await a.inject("/v1/patch")).json()).toEqual({
      patch: "16.19",
      dataPath: "data/x",
      dataDate: "2026-09-27",
      allMatches: 4242,
      checkedAt: 1234,
    });
  });

  it("gives each client address its own budget, answering 429 with Retry-After once it is spent", async () => {
    const { app: a, db } = await app({ requestsPerSecondPerIp: 1, burstPerIp: 2 });
    insertVersion(db);
    const from = (remoteAddress: string) => a.inject({ url: "/v1/patch", remoteAddress });

    expect((await from("203.0.113.7")).statusCode).toBe(200);
    expect((await from("203.0.113.7")).statusCode).toBe(200);
    const refused = await from("203.0.113.7");
    expect(refused.statusCode).toBe(429);
    expect(refused.headers["retry-after"]).toBe("1");
    expect(refused.json()).toEqual({ error: "too many requests", retryAfter: 1 });

    // Another address is untouched by the first one's spending.
    expect((await from("198.51.100.4")).statusCode).toBe(200);
  });

  it("takes the client from X-Forwarded-For only when Railway's proxy forwards the request", async () => {
    const { app: a, db } = await app({ requestsPerSecondPerIp: 1, burstPerIp: 1 });
    insertVersion(db);
    const via = (remoteAddress: string, forwardedFor: string) =>
      a.inject({ url: "/v1/patch", remoteAddress, headers: { "x-forwarded-for": forwardedFor } });

    // Through Railway's proxy: the leftmost address is the client, so two clients behind one proxy
    // address each get their own budget.
    expect((await via("100.64.0.2", "203.0.113.7")).statusCode).toBe(200);
    expect((await via("100.64.0.2", "198.51.100.4, 100.64.0.9")).statusCode).toBe(200);
    expect((await via("100.64.0.2", "203.0.113.7")).statusCode).toBe(429);

    // From anywhere else the header is ignored: a client cannot claim a fresh address with it.
    expect((await via("192.0.2.1", "10.0.0.1")).statusCode).toBe(200);
    expect((await via("192.0.2.1", "10.0.0.2")).statusCode).toBe(429);
  });

  it("keeps a service-wide ceiling over every address", async () => {
    const { app: a, db } = await app({ requestsPerSecond: 1 });
    insertVersion(db);

    // A burst of twice the rate, then a refusal.
    expect((await a.inject("/v1/patch")).statusCode).toBe(200);
    expect((await a.inject("/v1/patch")).statusCode).toBe(200);
    const refused = await a.inject("/v1/patch");
    expect(refused.statusCode).toBe(429);
    expect(refused.headers["retry-after"]).toBe("1");
    expect(refused.json()).toEqual({ error: "too many requests", retryAfter: 1 });
  });
});

describe("errors", () => {
  it("answer an unknown route as JSON", async () => {
    const { app: a } = await app();
    const response = await a.inject("/v1/nope");
    expect(response.statusCode).toBe(404);
    expect(response.json()).toEqual({ error: "not found: /v1/nope", retryAfter: null });
  });

  it("answer a store failure generically, without its details", async () => {
    const { app: a, db } = await app();
    db.close();
    const response = await a.inject("/v1/health");
    expect(response.statusCode).toBe(500);
    expect(response.json()).toEqual({ error: "internal error", retryAfter: null });
  });
});
