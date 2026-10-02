/** Shared test setup: throwaway databases, a fake upstream server, and the app wired over both. */

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import Fastify from "fastify";
import { pino } from "pino";
import { onTestFinished } from "vitest";
import { buildApp } from "../app.ts";
import { Data } from "../data.ts";
import { type Connection, nowUnix, openDatabase } from "../db/database.ts";
import { Store } from "../db/store.ts";
import { UpstreamClient } from "../upstream/client.ts";
import { Documents } from "../upstream/documents.ts";

export const silentLog = pino({ level: "silent" });

/** A fresh directory, removed when the test ends. */
export function tempDir(): string {
  const dir = mkdtempSync(join(tmpdir(), "aramkit-cache-"));
  onTestFinished(() => rmSync(dir, { recursive: true, force: true }));
  return dir;
}

/** A migrated database in a fresh directory, closed when the test ends. */
export function tempDatabase(): { db: Connection; path: string } {
  const path = join(tempDir(), "cache.db");
  const db = openDatabase(path);
  // Registered after `tempDir`'s cleanup, so it runs first: the file is closed before it is removed.
  onTestFinished(() => {
    db.close();
  });
  return { db, path };
}

/** Records one patch as `versions.json` would, as the latest. */
export function insertVersion(
  db: Connection,
  { version = "16.19", dataPath = "data/16.19-test", firstSeenAt = 1_000 } = {},
): void {
  db.prepare(
    `INSERT INTO versions (version, data_path, resource_path, data_date, build_time_ms,
                           all_matches, high_matches, is_latest, first_seen_at)
     VALUES (?, ?, '', '2026-09-27', 1, 4242, 0, 1, ?)`,
  ).run(version, dataPath, firstSeenAt);
}

/** What a fake upstream path answers: a JSON body, a raw string, or a bare status. */
export type FakeAnswer =
  | { json: unknown; delayMs?: number }
  | { text: string; delayMs?: number }
  | { status: number; delayMs?: number };

/**
 * A stand-in for aramkit and CommunityDragon on a random local port, counting hits per path.
 * Answers can be changed while it runs; unlisted paths are 404s.
 */
export async function fakeUpstream(answers: Record<string, FakeAnswer> = {}) {
  const hits = new Map<string, number>();
  const server = Fastify();
  server.get("/*", async (request, reply) => {
    const path = request.url.split("?")[0] ?? "";
    hits.set(path, (hits.get(path) ?? 0) + 1);
    const answer = answers[path] ?? { status: 404 };
    if (answer.delayMs) {
      await new Promise((r) => setTimeout(r, answer.delayMs));
    }
    if ("status" in answer) {
      return reply.status(answer.status).send("<html>nope</html>");
    }
    const body = "json" in answer ? JSON.stringify(answer.json) : answer.text;
    return reply.type("application/json").send(body);
  });
  const base = await server.listen({ host: "127.0.0.1", port: 0 });
  onTestFinished(() => server.close());
  return { base, answers, hits: (path: string) => hits.get(path) ?? 0 };
}

/** The whole service over a temporary database, pointed at `upstreamBase` for both hosts. */
export async function testService({
  upstreamBase = "http://127.0.0.1:9",
  requestsPerSecond = 0,
  requestsPerSecondPerIp = 0,
  burstPerIp = 0,
  adminToken = null as string | null,
} = {}) {
  const { db } = tempDatabase();
  const store = new Store(db);
  const client = new UpstreamClient({ userAgent: "test", concurrency: 4, log: silentLog });
  const documents = new Documents({
    store,
    client,
    aramkitBase: upstreamBase,
    cdragonBase: upstreamBase,
  });
  const data = new Data({ store, documents, log: silentLog });
  const app = await buildApp({
    config: { requestsPerSecond, requestsPerSecondPerIp, burstPerIp, adminToken },
    store,
    data,
    log: silentLog,
    startedAt: nowUnix() - 5,
  });
  onTestFinished(() => app.close());
  return { app, db, store, client, documents, data };
}
