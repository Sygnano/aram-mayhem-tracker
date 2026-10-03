/**
 * Entry point of the crawler (D-090), which Railway runs on a cron as its own service from this same
 * image: start command `node dist/scripts/crawl.js`, with `SERVICE_URL` and `ADMIN_TOKEN` set.
 * Locally, `node src/scripts/crawl.ts` with the same variables.
 *
 * Exits 0 when the current data version is complete, 1 when it could not get there. Railway skips a
 * scheduled run while the previous one is still going, and the next run resumes where this one
 * stopped, so a failure costs nothing but time.
 */

import { pino } from "pino";
import { crawlerConfigFromEnv } from "../config.ts";
import { runCrawl } from "../crawler.ts";

const log = pino({ level: process.env.LOG_LEVEL?.trim() || "info" });

try {
  const config = crawlerConfigFromEnv();
  log.info(
    { service: config.serviceUrl, aramkit: config.aramkitBase, intervalMs: config.intervalMs },
    "crawl starting",
  );
  await runCrawl({ config, log });
} catch (error) {
  log.error({ err: error }, "crawl failed");
  process.exitCode = 1;
}
