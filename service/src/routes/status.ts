/** `/v1/health` and `/v1/patch`: what the service holds, without fetching anything. */

import type { FastifyPluginAsyncTypebox } from "@fastify/type-provider-typebox";
import { Type } from "typebox";
import { nowUnix } from "../db/database.ts";
import type { LatestVersion, Store } from "../db/store.ts";
import { UnavailableError } from "../errors.ts";

export interface StatusOptions {
  store: Store;
  /**
   * When this process started, in Unix seconds, so `/v1/health` can tell "just deployed, still empty"
   * from "running for a day and still empty", which is a real fault.
   */
  startedAt: number;
}

const Nullable = <T extends Parameters<typeof Type.Union>[0][number]>(schema: T) =>
  Type.Union([schema, Type.Null()]);

const Health = Type.Object({
  ok: Type.Boolean(),
  /** aramkit patch label, e.g. "16.19". `null` before the first successful `versions.json` read. */
  patch: Nullable(Type.String()),
  dataDate: Nullable(Type.String()),
  /** Seconds since we first saw the current build. Large and growing means the poller is stuck. */
  ageSeconds: Nullable(Type.Integer()),
  championsCached: Type.Integer(),
  uptimeSeconds: Type.Integer(),
});

const Patch = Type.Object({
  patch: Type.String(),
  dataPath: Type.String(),
  dataDate: Type.String(),
  allMatches: Type.Integer(),
  checkedAt: Type.Integer(),
});

/** Health, which is outside the rate limit: Railway's check must not fail because the service is busy. */
export const healthRoute: FastifyPluginAsyncTypebox<StatusOptions> = async (
  app,
  { store, startedAt },
) => {
  /**
   * Railway's health check points here. It reports `ok` on a reachable database, not on a warm
   * cache: an empty store on a fresh deploy is correct, and failing the check would refuse to roll
   * out.
   */
  app.get("/v1/health", { schema: { response: { 200: Health } } }, async () => {
    store.ping();
    const latest = store.latestVersion();
    const now = nowUnix();
    return {
      ok: true,
      patch: latest?.version ?? null,
      dataDate: latest?.dataDate ?? null,
      ageSeconds: latest ? now - latest.firstSeenAt : null,
      championsCached: latest ? store.championsCached(latest.dataPath) : 0,
      uptimeSeconds: now - startedAt,
    };
  });
};

export const patchRoute: FastifyPluginAsyncTypebox<StatusOptions> = async (app, { store }) => {
  app.get("/v1/patch", { schema: { response: { 200: Patch } } }, async () => {
    const latest = currentVersion(store);
    return {
      patch: latest.version,
      dataPath: latest.dataPath,
      dataDate: latest.dataDate,
      allMatches: latest.allMatches,
      checkedAt: latest.firstSeenAt,
    };
  });
};

/** The patch every data endpoint answers for, or a 503 before the first `versions.json` read. */
export function currentVersion(store: Store): LatestVersion {
  const latest = store.latestVersion();
  if (latest === null) {
    throw new UnavailableError("no patch has been fetched yet");
  }
  return latest;
}
