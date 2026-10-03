/**
 * The crawler (D-090): fetches aramkit's current data version one document at a time, slowly, and
 * sends each to the web service, which stores it. `scripts/crawl.ts` runs this from a Railway cron.
 *
 * It holds no state of its own. The service says what is missing after every document, so a run that
 * stops halfway, cut short by a deploy or by aramkit failing, is picked up by the next one where it
 * left off. When nothing has changed upstream, a run costs one request to aramkit and ends.
 */

import { setTimeout as sleep } from "node:timers/promises";
import type { Logger } from "pino";
import type { CrawlerConfig } from "./config.ts";
import type { CrawlStatus } from "./crawl.ts";
import { upstreamStatus } from "./errors.ts";
import { UpstreamClient } from "./upstream/client.ts";

export interface CrawlerOptions {
  config: CrawlerConfig;
  log: Logger;
  /** How the crawler waits between requests. Tests pass one that does not. */
  wait?: (ms: number) => Promise<unknown>;
}

export interface CrawlOutcome {
  dataPath: string | null;
  /** Documents fetched from aramkit this run, `versions.json` included. */
  fetched: number;
  complete: boolean;
}

/** Runs one crawl. Resolves when the version is complete; rejects when it cannot get further. */
export async function runCrawl({
  config,
  log,
  wait = sleep,
}: CrawlerOptions): Promise<CrawlOutcome> {
  // One request at a time, whatever happens: the pace is the point.
  const upstream = new UpstreamClient({ userAgent: config.userAgent, concurrency: 1, log });
  const service = serviceClient(config);

  const versions = await upstream.get(`${config.aramkitBase}/data/versions.json`);
  let fetched = 1;
  let status = await service("POST", "/admin/crawl/versions", versions.body);
  log.info(
    {
      dataPath: status.dataPath,
      done: status.done,
      expected: status.expected,
      complete: status.complete,
    },
    "crawl status",
  );

  let failures = 0;
  while (!status.complete) {
    if (status.blocked !== null) {
      throw new Error(`the crawl cannot complete: ${status.blocked}`);
    }
    const next = status.missing[0];
    if (next === undefined || status.dataPath === null) {
      throw new Error("the service reports nothing missing, yet the crawl is not complete");
    }
    await wait(config.intervalMs);

    const query = new URLSearchParams({
      dataPath: status.dataPath,
      kind: next.kind,
      key: next.key,
    });
    try {
      let body: Buffer | null;
      try {
        body = (await upstream.get(`${config.aramkitBase}/${next.path}`)).body;
      } catch (error) {
        if (upstreamStatus(error) !== 404) {
          throw error;
        }
        body = null;
      } finally {
        fetched += 1;
      }
      status =
        body === null
          ? await service("PUT", `/admin/crawl/absent?${query}`)
          : await service("PUT", `/admin/crawl/doc?${query}`, body);
      failures = 0;
      if (status.done % 25 === 0 || status.complete) {
        log.info(
          { dataPath: status.dataPath, done: status.done, expected: status.expected },
          "crawl progress",
        );
      }
    } catch (error) {
      failures += 1;
      log.warn({ err: error, path: next.path, failures }, "could not crawl a document");
      if (failures >= config.maxFailures) {
        throw new Error(
          `giving up after ${failures} failures in a row; the next run resumes from here`,
          {
            cause: error,
          },
        );
      }
    }
  }

  log.info({ dataPath: status.dataPath, fetched }, "crawl complete");
  return { dataPath: status.dataPath, fetched, complete: true };
}

/** Calls one crawl route on the web service and returns the status it answers with. */
function serviceClient({ serviceUrl, adminToken }: CrawlerConfig) {
  return async (method: "POST" | "PUT", path: string, body?: Buffer): Promise<CrawlStatus> => {
    const response = await fetch(`${serviceUrl}${path}`, {
      method,
      headers: {
        authorization: `Bearer ${adminToken}`,
        ...(body === undefined ? {} : { "content-type": "application/octet-stream" }),
      },
      body,
      signal: AbortSignal.timeout(60_000),
    });
    const text = await response.text();
    if (!response.ok) {
      throw new Error(
        `${method} ${path}: the service answered ${response.status}: ${text.slice(0, 300)}`,
      );
    }
    return JSON.parse(text) as CrawlStatus;
  };
}
