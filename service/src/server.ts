/**
 * The aramkit caching service.
 *
 * Answers from its own store only. aramkit is crawled ahead of time by `scripts/crawl.ts`, which runs
 * as a separate Railway cron service and sends what it fetches to `/admin/crawl` (D-090); a data
 * version is served once all of it is here. CommunityDragon is still fetched on a miss.
 */

import { pino } from "pino";
import { buildApp } from "./app.ts";
import { configFromEnv } from "./config.ts";
import { Crawl } from "./crawl.ts";
import { Data } from "./data.ts";
import { Datasets } from "./dataset.ts";
import { nowUnix, openDatabase } from "./db/database.ts";
import { Store } from "./db/store.ts";
import { UpstreamClient } from "./upstream/client.ts";
import { Documents } from "./upstream/documents.ts";

const config = configFromEnv();
const log = pino({ level: config.logLevel });

log.info(
  {
    port: config.port,
    database: config.databasePath,
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
const documents = new Documents({ store, client, cdragonBase: config.cdragonBase });
const data = new Data({ store, documents, log });
const datasets = new Datasets({ store, data, log });
const crawl = new Crawl({ store, data, log, onComplete: () => datasets.warm() });
const app = await buildApp({ config, store, data, crawl, datasets, log, startedAt: nowUnix() });

// Built before the first app asks, so a deploy does not make the first download wait on it.
if (store.latestVersion() !== null) {
  datasets.warm();
}

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
