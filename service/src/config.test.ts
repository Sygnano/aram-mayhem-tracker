import { describe, expect, it } from "vitest";
import { configFromEnv, crawlerConfigFromEnv } from "./config.ts";

describe("configuration", () => {
  it("starts with usable defaults when nothing is set", () => {
    const config = configFromEnv({});
    expect(config).toMatchObject({
      port: 8080,
      databasePath: "/data/cache.db",
      aramkitBase: "https://data.aramkit.com",
      cdragonBase: "https://raw.communitydragon.org",
      upstreamConcurrency: 4,
      adminToken: null,
      requestsPerSecondPerIp: 1,
      burstPerIp: 20,
      requestsPerSecond: 200,
      logLevel: "info",
    });
    expect(config.userAgent).toMatch(
      /^aram-mayhem-tracker\/\d+\.\d+\.\d+ \(\+https:\/\/github\.com\/Sygnano\/aram-mayhem-tracker\)$/,
    );
  });

  it("reads what is set, trimming trailing slashes and blank tokens", () => {
    const config = configFromEnv({
      PORT: "8391",
      ARAMKIT_BASE: "http://127.0.0.1:9000/",
      ADMIN_TOKEN: "  s3cret  ",
      REQUESTS_PER_SECOND: "0",
      REQUESTS_PER_SECOND_PER_IP: "3",
      BURST_PER_IP: "6",
    });
    expect(config.port).toBe(8391);
    expect(config.aramkitBase).toBe("http://127.0.0.1:9000");
    expect(config.adminToken).toBe("s3cret");
    expect(config.requestsPerSecond).toBe(0);
    expect(config.requestsPerSecondPerIp).toBe(3);
    expect(config.burstPerIp).toBe(6);

    expect(configFromEnv({ ADMIN_TOKEN: "   " }).adminToken).toBeNull();
    expect(configFromEnv({ PORT: "  " }).port, "blank means unset").toBe(8080);
  });

  it("refuses a value that is set but malformed", () => {
    expect(() => configFromEnv({ PORT: "eighty" })).toThrow(/PORT="eighty" is not a valid value/);
    expect(() => configFromEnv({ PORT: "70000" })).toThrow(/PORT/);
    expect(() => configFromEnv({ UPSTREAM_CONCURRENCY: "-1" })).toThrow(/UPSTREAM_CONCURRENCY/);
    expect(() => configFromEnv({ REQUESTS_PER_SECOND: "1.5" })).toThrow(/REQUESTS_PER_SECOND/);
  });

  it("gives the crawler a slow pace, and refuses to start it without a service or a token", () => {
    const env = {
      SERVICE_URL: "http://aramkit-cache.railway.internal:8080/",
      ADMIN_TOKEN: "s3cret",
    };
    expect(crawlerConfigFromEnv(env)).toMatchObject({
      serviceUrl: "http://aramkit-cache.railway.internal:8080",
      adminToken: "s3cret",
      aramkitBase: "https://data.aramkit.com",
      intervalMs: 5_000,
      maxFailures: 5,
    });
    expect(crawlerConfigFromEnv({ ...env, CRAWL_INTERVAL_SECS: "10" }).intervalMs).toBe(10_000);
    expect(() => crawlerConfigFromEnv({ ADMIN_TOKEN: "s3cret" })).toThrow(/SERVICE_URL/);
    expect(() => crawlerConfigFromEnv({ SERVICE_URL: "http://x" })).toThrow(/ADMIN_TOKEN/);
    expect(() => crawlerConfigFromEnv({ ...env, CRAWL_INTERVAL_SECS: "1.5" })).toThrow(
      /CRAWL_INTERVAL_SECS/,
    );
  });
});
