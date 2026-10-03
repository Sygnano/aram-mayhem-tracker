/**
 * Configuration, entirely from the environment so Railway can set it without a rebuild. Nothing is
 * required: every value has a default the service starts with.
 */

import { readFileSync } from "node:fs";

/** `src/` and `dist/` both sit directly under the folder holding package.json. */
const packageVersion: string = JSON.parse(
  readFileSync(new URL("../package.json", import.meta.url), "utf8"),
).version;

export interface Config {
  /** Railway injects `PORT`; we bind `0.0.0.0` on it. */
  port: number;
  /** SQLite file. On Railway this lives on the mounted volume, so it survives deploys. */
  databasePath: string;
  /**
   * Root of aramkit's static data host, which only the crawler fetches from. Overridable so tests
   * can point at a local fixture server.
   */
  aramkitBase: string;
  /** Root of the CommunityDragon host, used for augment rarity, names and icons. */
  cdragonBase: string;
  /**
   * Sent on every upstream request. Identifies the app and gives a contact address, because the
   * aramkit developer approved this use and should be able to reach us if it misbehaves.
   */
  userAgent: string;
  /** Upper bound on concurrent upstream fetches. */
  upstreamConcurrency: number;
  /**
   * Bearer token the `/admin` editor must send to save the anvil rankings. Unset means
   * saving is refused outright, so a deploy that forgets it is locked rather than open.
   */
  adminToken: string | null;
  /**
   * Sustained requests per second the data endpoints accept from one client address, with bursts of
   * up to `burstPerIp`. `0` switches the per-address limit off.
   */
  requestsPerSecondPerIp: number;
  /** How many requests one client address may make at once before the sustained rate applies. */
  burstPerIp: number;
  /**
   * Sustained requests per second across all callers, with bursts of twice that: a ceiling over
   * the per-address limits. `0` switches it off.
   */
  requestsPerSecond: number;
  /** pino level: `fatal`, `error`, `warn`, `info`, `debug`, `trace` or `silent`. */
  logLevel: string;
}

type Env = Record<string, string | undefined>;

/** Reads the configuration, throwing on a value that is set but malformed. */
export function configFromEnv(env: Env = process.env): Config {
  const adminToken = env.ADMIN_TOKEN?.trim() ?? "";
  return {
    port: intEnv(env, "PORT", 8080, { max: 65535 }),
    databasePath: stringEnv(env, "DATABASE_PATH", "/data/cache.db"),
    aramkitBase: trimSlashes(stringEnv(env, "ARAMKIT_BASE", "https://data.aramkit.com")),
    cdragonBase: trimSlashes(stringEnv(env, "CDRAGON_BASE", "https://raw.communitydragon.org")),
    userAgent: stringEnv(
      env,
      "UPSTREAM_USER_AGENT",
      `aram-mayhem-tracker/${packageVersion} (+https://github.com/Sygnano/aram-mayhem-tracker)`,
    ),
    upstreamConcurrency: intEnv(env, "UPSTREAM_CONCURRENCY", 4),
    adminToken: adminToken === "" ? null : adminToken,
    requestsPerSecondPerIp: intEnv(env, "REQUESTS_PER_SECOND_PER_IP", 1),
    burstPerIp: intEnv(env, "BURST_PER_IP", 20),
    requestsPerSecond: intEnv(env, "REQUESTS_PER_SECOND", 200),
    logLevel: stringEnv(env, "LOG_LEVEL", "info"),
  };
}

/** The crawler's configuration (`scripts/crawl.ts`), from the same environment conventions. */
export interface CrawlerConfig {
  /**
   * The web service to send documents to. On Railway, its private address, such as
   * `http://aramkit-cache.railway.internal:8080`. Required.
   */
  serviceUrl: string;
  /** The web service's `ADMIN_TOKEN`. Required: every crawl route needs it. */
  adminToken: string;
  aramkitBase: string;
  userAgent: string;
  /** Time between two requests to aramkit. Five seconds unless set. */
  intervalMs: number;
  /** Failures in a row before the run gives up; the next cron run picks up where it stopped. */
  maxFailures: number;
}

export function crawlerConfigFromEnv(env: Env = process.env): CrawlerConfig {
  const service = configFromEnv({ ...env, PORT: undefined });
  const serviceUrl = trimSlashes(stringEnv(env, "SERVICE_URL", ""));
  if (serviceUrl === "") {
    throw new Error("SERVICE_URL is not set: the crawler needs the web service's address");
  }
  if (service.adminToken === null) {
    throw new Error("ADMIN_TOKEN is not set: the crawler needs the web service's admin token");
  }
  return {
    serviceUrl,
    adminToken: service.adminToken,
    aramkitBase: service.aramkitBase,
    userAgent: service.userAgent,
    intervalMs: intEnv(env, "CRAWL_INTERVAL_SECS", 5) * 1000,
    maxFailures: intEnv(env, "CRAWL_MAX_FAILURES", 5),
  };
}

function stringEnv(env: Env, key: string, fallback: string): string {
  const value = env[key];
  return value === undefined || value.trim() === "" ? fallback : value;
}

/** A non-negative integer, as the Rust version's unsigned fields were. */
function intEnv(env: Env, key: string, fallback: number, { max = Number.MAX_SAFE_INTEGER } = {}) {
  const raw = env[key]?.trim();
  if (raw === undefined || raw === "") {
    return fallback;
  }
  const value = Number(raw);
  if (!/^\d+$/.test(raw) || !Number.isSafeInteger(value) || value > max) {
    throw new Error(
      `${key}=${JSON.stringify(raw)} is not a valid value: expected an integer 0..${max}`,
    );
  }
  return value;
}

function trimSlashes(url: string): string {
  return url.replace(/\/+$/, "");
}
