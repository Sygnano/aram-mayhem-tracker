/**
 * `GET /v1/dataset`: everything about every champion, for the app to keep on disk (D-090).
 *
 * The body is stored gzipped and sent as stored, so it bypasses both the response schema and
 * `@fastify/compress`. Its shape is described in `src/dataset.ts` and mirrored by the app's
 * `aramkit-client` crate. Send `If-None-Match` with the ETag of the copy you hold to get a 304 when
 * nothing changed.
 */

import { gunzipSync } from "node:zlib";
import type { FastifyPluginAsyncTypebox } from "@fastify/type-provider-typebox";
import type { Datasets } from "../dataset.ts";

export interface DatasetRouteOptions {
  datasets: Datasets;
}

export const datasetRoutes: FastifyPluginAsyncTypebox<DatasetRouteOptions> = async (
  app,
  { datasets },
) => {
  app.get("/v1/dataset", { compress: false }, async (request, reply) => {
    // Answered before anything is built: an app checking for an update costs a hash.
    const etag = datasets.currentEtag();
    reply.header("etag", etag).header("cache-control", "no-cache");
    if (request.headers["if-none-match"] === etag) {
      return reply.status(304).send();
    }

    const built = await datasets.get();
    reply.header("etag", built.etag).type("application/json; charset=utf-8");
    // Every client this is meant for accepts gzip; anything else gets it unpacked, slowly.
    if (/\bgzip\b/.test(request.headers["accept-encoding"] ?? "")) {
      return reply
        .header("content-encoding", "gzip")
        .header("vary", "accept-encoding")
        .send(built.gzip);
    }
    return reply.header("vary", "accept-encoding").send(gunzipSync(built.gzip));
  });
};
