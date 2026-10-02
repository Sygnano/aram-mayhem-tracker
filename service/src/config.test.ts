import { describe, expect, it } from "vitest";
import { configFromEnv } from "./config.ts";

describe("configuration", () => {
  it("starts with usable defaults when nothing is set", () => {
    const config = configFromEnv({});
    expect(config).toMatchObject({
      port: 8080,
      databasePath: "/data/cache.db",
      aramkitBase: "https://data.aramkit.com",
      cdragonBase: "https://raw.communitydragon.org",
      versionsPollMs: 6 * 60 * 60 * 1000,
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
    expect(() => configFromEnv({ VERSIONS_POLL_SECS: "1.5" })).toThrow(/VERSIONS_POLL_SECS/);
  });
});
