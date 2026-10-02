/**
 * Token buckets: one per client address, and one for the whole service above them.
 *
 * The per-address bucket is what a user meets. One app makes a handful of requests per champ select
 * (the champion table, the shard names, one bundle per champion, one damage split per game), so a
 * small sustained rate with room for a burst never refuses it, while one caller looping on an
 * endpoint is held to that rate without taking anybody else's budget.
 *
 * The service-wide bucket is a ceiling under everything, there for a flood from many addresses at
 * once. What it ultimately protects is aramkit, who granted this project access.
 */

import { BlockList, isIP } from "node:net";

export class RateLimiter {
  readonly #perSecond: number;
  readonly #burst: number;
  #tokens: number;
  /** When `#tokens` was last brought up to date, in milliseconds. */
  #last: number;

  /** `perSecond` sustained, `burst` at once (twice the rate unless given). Zero never refuses. */
  constructor(perSecond: number, now: number = performance.now(), burst: number = perSecond * 2) {
    this.#perSecond = perSecond;
    this.#burst = Math.max(burst, 1);
    this.#tokens = this.#burst;
    this.#last = now;
  }

  /** Takes one token if there is one. `now` is in milliseconds, from a monotonic clock. */
  tryTake(now: number = performance.now()): boolean {
    if (this.#perSecond <= 0) {
      return true;
    }
    const tokens = this.#tokensAt(now);
    const allowed = tokens >= 1;
    this.#tokens = allowed ? tokens - 1 : tokens;
    this.#last = Math.max(now, this.#last);
    return allowed;
  }

  /** Whether the bucket would be full at `now`, so forgetting it changes nothing. */
  isFull(now: number = performance.now()): boolean {
    return this.#tokensAt(now) >= this.#burst;
  }

  #tokensAt(now: number): number {
    const elapsedSeconds = Math.max(0, now - this.#last) / 1000;
    return Math.min(this.#tokens + elapsedSeconds * this.#perSecond, this.#burst);
  }
}

/** One bucket per key (a client address), each `perSecond` sustained with a burst of `burst`. */
export class KeyedRateLimiter {
  readonly #perSecond: number;
  readonly #burst: number;
  readonly #buckets = new Map<string, RateLimiter>();
  #lastSweep: number;

  /** Zero never refuses. */
  constructor(perSecond: number, burst: number, now: number = performance.now()) {
    this.#perSecond = perSecond;
    this.#burst = burst;
    this.#lastSweep = now;
  }

  tryTake(key: string, now: number = performance.now()): boolean {
    if (this.#perSecond <= 0) {
      return true;
    }
    this.#sweep(now);
    let bucket = this.#buckets.get(key);
    if (bucket === undefined) {
      bucket = new RateLimiter(this.#perSecond, now, this.#burst);
      this.#buckets.set(key, bucket);
    }
    return bucket.tryTake(now);
  }

  /** How many addresses are being tracked. */
  get size(): number {
    return this.#buckets.size;
  }

  /**
   * Forgets every bucket that has filled up again, at most once a minute. A full bucket is exactly
   * what a new one would be, so this only bounds memory: an address that went quiet costs nothing.
   */
  #sweep(now: number): void {
    if (now - this.#lastSweep < 60_000) {
      return;
    }
    this.#lastSweep = now;
    for (const [key, bucket] of this.#buckets) {
      if (bucket.isFull(now)) {
        this.#buckets.delete(key);
      }
    }
  }
}

/**
 * Railway's edge proxies reach the service from this range, and only they do. Staff guidance
 * (2026-03-09): the edge strips any `X-Forwarded-For` a client sends and writes its own, so the
 * leftmost entry is the connecting client. `X-Real-IP` is not used: it holds a CDN address when
 * Railway's CDN is in the path.
 */
const RAILWAY_PROXIES = new BlockList();
RAILWAY_PROXIES.addSubnet("100.0.0.0", 8, "ipv4");

/**
 * Fastify's `trustProxy`: the forwarded chain is believed only when the connection itself comes from
 * Railway's proxy, and then all of it, so `request.ip` is the leftmost address. Anywhere else (a
 * local run, a test) it is the socket's own address, and a forged header changes nothing.
 */
export function trustRailwayProxy(address: string, hop: number): boolean {
  if (hop > 0) {
    return true;
  }
  const v4 = address.startsWith("::ffff:") ? address.slice(7) : address;
  return isIP(v4) === 4 && RAILWAY_PROXIES.check(v4, "ipv4");
}
