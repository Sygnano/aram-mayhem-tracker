/** One GET against aramkit or CommunityDragon, with the concurrency cap, a timeout and retries. */

import { setTimeout as sleep } from "node:timers/promises";
import pLimit, { type LimitFunction } from "p-limit";
import type { Logger } from "pino";
import { UpstreamStatusError } from "../errors.ts";

/** The whole request, headers and body. 30 s matches what the Rust client allowed. */
const TIMEOUT_MS = 30_000;
const ATTEMPTS = 3;
/** Before the second attempt; doubled before the third. */
const FIRST_BACKOFF_MS = 250;

export interface Fetched {
  etag: string | null;
  body: Buffer;
}

export interface UpstreamClientOptions {
  userAgent: string;
  /** Upper bound on simultaneous upstream requests, so a burst of cold champions stays polite. */
  concurrency: number;
  log: Logger;
}

export class UpstreamClient {
  readonly #userAgent: string;
  readonly #limit: LimitFunction;
  readonly #log: Logger;

  constructor({ userAgent, concurrency, log }: UpstreamClientOptions) {
    this.#userAgent = userAgent;
    this.#limit = pLimit(Math.max(1, concurrency));
    this.#log = log;
  }

  /**
   * GETs `url`, retrying transport errors, 429 and 5xx. A 4xx is a fact about an immutable path, so
   * it is never retried. The concurrency slot is held across the retries.
   */
  get(url: string): Promise<Fetched> {
    return this.#limit(async () => {
      for (let attempt = 1; ; attempt++) {
        try {
          return await this.#getOnce(url);
        } catch (error) {
          const status = error instanceof UpstreamStatusError ? error.status : null;
          const retryable = status === null || status === 429 || status >= 500;
          if (!retryable || attempt >= ATTEMPTS) {
            throw error;
          }
          this.#log.warn({ url, attempt, err: error }, "upstream fetch failed, retrying");
          await sleep(FIRST_BACKOFF_MS * 2 ** (attempt - 1));
        }
      }
    });
  }

  async #getOnce(url: string): Promise<Fetched> {
    let response: Response;
    try {
      response = await fetch(url, {
        headers: { "user-agent": this.#userAgent },
        signal: AbortSignal.timeout(TIMEOUT_MS),
      });
    } catch (error) {
      throw new Error(`GET ${url}: ${describe(error)}`, { cause: error });
    }
    if (!response.ok) {
      // A 404 body is 27 KB of HTML, so the status is what we keep. Cancelling frees the connection.
      await response.body?.cancel();
      throw new UpstreamStatusError(response.status, url);
    }
    try {
      return {
        etag: response.headers.get("etag"),
        body: Buffer.from(await response.arrayBuffer()),
      };
    } catch (error) {
      throw new Error(`reading the body of ${url}: ${describe(error)}`, { cause: error });
    }
  }
}

function describe(error: unknown): string {
  if (error instanceof Error) {
    // undici reports a refused connection as "fetch failed" with the reason in `cause`.
    return error.cause instanceof Error
      ? `${error.message} (${error.cause.message})`
      : error.message;
  }
  return String(error);
}
