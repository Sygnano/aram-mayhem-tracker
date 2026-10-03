/**
 * Builds the HTTP app from its dependencies. `server.ts` calls this to serve; tests call it over a
 * temporary database and a local fake upstream, with nothing mocked.
 */

import compress from "@fastify/compress";
import type { TypeBoxTypeProvider } from "@fastify/type-provider-typebox";
import Fastify, {
  type FastifyBaseLogger,
  type FastifyInstance,
  type FastifyPluginAsync,
} from "fastify";
import type { Config } from "./config.ts";
import type { Crawl } from "./crawl.ts";
import type { Data } from "./data.ts";
import type { Datasets } from "./dataset.ts";
import type { Store } from "./db/store.ts";
import { TooManyRequestsError, toErrorResponse } from "./errors.ts";
import { KeyedRateLimiter, RateLimiter, trustRailwayProxy } from "./rate-limit.ts";
import { adminRoute, anvilRoutes } from "./routes/anvils.ts";
import { augmentRoutes } from "./routes/augments.ts";
import { bundleRoutes } from "./routes/bundle.ts";
import { championRoutes } from "./routes/champions.ts";
import { crawlRoutes } from "./routes/crawl.ts";
import { datasetRoutes } from "./routes/dataset.ts";
import { healthRoute, patchRoute } from "./routes/status.ts";

export interface AppDeps {
  config: Pick<
    Config,
    "requestsPerSecond" | "requestsPerSecondPerIp" | "burstPerIp" | "adminToken"
  >;
  store: Store;
  data: Data;
  crawl: Crawl;
  datasets: Datasets;
  log: FastifyBaseLogger;
  /** When this process started, in Unix seconds. */
  startedAt: number;
}

export async function buildApp({
  config,
  store,
  data,
  crawl,
  datasets,
  log,
  startedAt,
}: AppDeps): Promise<FastifyInstance> {
  const app = Fastify({
    loggerInstance: log,
    // Railway's proxy closes idle connections itself; this only stops a stalled client holding one.
    requestTimeout: 60_000,
    // `request.ip` is the client behind Railway's proxy, and the socket address anywhere else.
    trustProxy: trustRailwayProxy,
  }).withTypeProvider<TypeBoxTypeProvider>();

  await app.register(compress);

  app.addHook("onSend", async (_request, reply) => {
    reply.header("x-content-type-options", "nosniff");
  });

  app.setErrorHandler((error, request, reply) => {
    const { statusCode, body, internal } = toErrorResponse(error);
    if (internal) {
      // It may carry details about the store or upstream, so it is logged in full and answered
      // generically.
      request.log.error({ err: error }, "request failed");
    }
    if (body.retryAfter !== null) {
      reply.header("retry-after", String(body.retryAfter));
    }
    return reply.status(statusCode).send(body);
  });

  app.setNotFoundHandler((request, reply) => {
    return reply.status(404).send({ error: `not found: ${request.url}`, retryAfter: null });
  });

  await app.register(healthRoute, { store, startedAt });
  await app.register(adminRoute);
  // The crawler sends a document every five seconds or so, with the admin token: no limit to apply.
  await app.register(crawlRoutes, { crawl, adminToken: config.adminToken });

  // Everything below is limited per client address (`REQUESTS_PER_SECOND_PER_IP`, `BURST_PER_IP`),
  // under one ceiling for the whole service (`REQUESTS_PER_SECOND`). The hook belongs to this
  // plugin's scope only, so health and `/admin` stay outside it by construction. The address is
  // checked first, so a client over its own limit does not spend the shared budget.
  const perIp = new KeyedRateLimiter(config.requestsPerSecondPerIp, config.burstPerIp);
  const ceiling = new RateLimiter(config.requestsPerSecond);
  const limitedRoutes: FastifyPluginAsync = async (limited) => {
    limited.addHook("onRequest", async (request) => {
      if (!perIp.tryTake(request.ip) || !ceiling.tryTake()) {
        throw new TooManyRequestsError();
      }
    });
    await limited.register(patchRoute, { store, startedAt });
    await limited.register(augmentRoutes, { store, data });
    await limited.register(championRoutes, { store, data });
    await limited.register(bundleRoutes, { store, data });
    await limited.register(datasetRoutes, { datasets });
    await limited.register(anvilRoutes, { store, data, adminToken: config.adminToken });
  };
  await app.register(limitedRoutes);

  return app;
}
