/**
 * The aramkit caching service.
 *
 * Reads its own store first and falls back to aramkit on a miss. Everything under an aramkit
 * `dataPath` is immutable, so the cache never expires; only `versions.json` is polled.
 */

import { pino } from "pino";
import { buildApp } from "./app.ts";
import { configFromEnv } from "./config.ts";
import { Data } from "./data.ts";
import { nowUnix, openDatabase } from "./db/database.ts";
import { Store } from "./db/store.ts";
import { UpstreamClient } from "./upstream/client.ts";
import { Documents } from "./upstream/documents.ts";
import { pollVersions } from "./upstream/versions.ts";

const config = configFromEnv();
const log = pino({ level: config.logLevel });

log.info(
  {
    port: config.port,
    database: config.databasePath,
    aramkit: config.aramkitBase,
    adminSaving: config.adminToken !== null,
    requestsPerSecond: config.requestsPerSecond,
    requestsPerSecondPerIp: config.requestsPerSecondPerIp,
    burstPerIp: config.burstPerIp,
  },
  "starting aramkit-cache",
);

const db = openDatabase(config.databasePath, {
  onMigrated: (version) => log.info({ version }, "applied schema migration"),
});
const store = new Store(db);
const client = new UpstreamClient({
  userAgent: config.userAgent,
  concurrency: config.upstreamConcurrency,
  log,
});
const documents = new Documents({
  store,
  client,
  aramkitBase: config.aramkitBase,
  cdragonBase: config.cdragonBase,
});
const data = new Data({ store, documents, log });
const app = await buildApp({ config, store, data, log, startedAt: nowUnix() });

const poller = pollVersions({
  client,
  store,
  aramkitBase: config.aramkitBase,
  everyMs: config.versionsPollMs,
  log,
});

// Railway sends SIGTERM on redeploy. Closing lets requests in flight finish before the database
// is closed under them.
let closing = false;
for (const signal of ["SIGTERM", "SIGINT"] as const) {
  process.once(signal, () => {
    if (closing) {
      return;
    }
    closing = true;
    log.info(`${signal} received, shutting down`);
    poller.stop();
    app
      .close()
      .then(() => {
        db.close();
        log.info("shut down cleanly");
      })
      .catch((error: unknown) => {
        log.error({ err: error }, "shutdown failed");
        process.exitCode = 1;
      });
  });
}

await app.listen({ host: "0.0.0.0", port: config.port });
