import { describe, expect, it } from "vitest";
import { KeyedRateLimiter, RateLimiter, trustRailwayProxy } from "./rate-limit.ts";

const times = (n: number, take: () => boolean) => Array.from({ length: n }, take);

describe("the rate limiter", () => {
  it("allows a burst, then refills at the sustained rate", () => {
    const start = 0;
    const limiter = new RateLimiter(5, start);

    // Twice the rate in one go, then nothing.
    expect(times(10, () => limiter.tryTake(start)).every(Boolean)).toBe(true);
    expect(limiter.tryTake(start)).toBe(false);

    // A second later, five more and no more than five.
    const later = start + 1_000;
    expect(times(5, () => limiter.tryTake(later)).every(Boolean)).toBe(true);
    expect(limiter.tryTake(later)).toBe(false);

    // A long quiet spell fills it to the burst, not beyond.
    const muchLater = later + 3_600_000;
    expect(times(50, () => limiter.tryTake(muchLater)).filter(Boolean)).toHaveLength(10);
  });

  it("treats a limit of zero as no limit", () => {
    const limiter = new RateLimiter(0, 0);
    expect(times(10_000, () => limiter.tryTake(0)).every(Boolean)).toBe(true);
  });
});

describe("the per-address rate limiter", () => {
  it("gives each key its own burst and rate", () => {
    const limiter = new KeyedRateLimiter(1, 3, 0);
    expect(times(3, () => limiter.tryTake("a", 0)).every(Boolean)).toBe(true);
    expect(limiter.tryTake("a", 0)).toBe(false);
    expect(limiter.tryTake("b", 0), "another key is untouched").toBe(true);
    expect(limiter.tryTake("a", 1_000), "one more a second later").toBe(true);
    expect(limiter.tryTake("a", 1_000)).toBe(false);
  });

  it("forgets keys whose bucket has filled up again", () => {
    const limiter = new KeyedRateLimiter(1, 3, 0);
    limiter.tryTake("quiet", 0);
    limiter.tryTake("busy", 0);
    expect(limiter.size).toBe(2);

    // A minute on, both have refilled; the sweep drops them before "busy" makes a new one.
    limiter.tryTake("busy", 61_000);
    expect(limiter.size).toBe(1);
  });

  it("treats a limit of zero as no limit", () => {
    const limiter = new KeyedRateLimiter(0, 0, 0);
    expect(times(1_000, () => limiter.tryTake("a", 0)).every(Boolean)).toBe(true);
    expect(limiter.size).toBe(0);
  });
});

describe("the trusted proxy", () => {
  it("is Railway's internal range, in either address form", () => {
    expect(trustRailwayProxy("100.64.0.2", 0)).toBe(true);
    expect(trustRailwayProxy("::ffff:100.64.0.2", 0)).toBe(true);
    expect(trustRailwayProxy("127.0.0.1", 0)).toBe(false);
    expect(trustRailwayProxy("::1", 0)).toBe(false);
    expect(trustRailwayProxy("203.0.113.7", 0)).toBe(false);
  });

  it("believes the rest of the chain once the connection is the proxy's", () => {
    expect(trustRailwayProxy("203.0.113.7", 1)).toBe(true);
  });
});
