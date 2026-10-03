/**
 * The crawler's routes (D-090), all behind `ADMIN_TOKEN` and outside the rate limit.
 *
 * - `GET /admin/crawl`: where the crawl stands and what is left.
 * - `POST /admin/crawl/versions`: a `versions.json` body, which picks the version to crawl.
 * - `PUT /admin/crawl/doc?dataPath=&kind=&key=`: one aramkit document, as aramkit sent it.
 * - `PUT /admin/crawl/absent?dataPath=&kind=&key=`: aramkit answered 404 for that document.
 *
 * Bodies are `application/octet-stream`, stored byte for byte. Every route answers with the crawl's
 * status, so the crawler always knows what to fetch next.
 */

import type { FastifyPluginAsyncTypebox } from "@fastify/type-provider-typebox";
import { Type } from "typebox";
import type { Crawl } from "../crawl.ts";
import { BadRequestError } from "../errors.ts";
import { authorize } from "./anvils.ts";
import { Nullable } from "./schemas.ts";

export interface CrawlRouteOptions {
  crawl: Crawl;
  adminToken: string | null;
}

/** A champion-details document is about 0.5 MB on 16.19; this leaves room for growth. */
const BODY_LIMIT = 32 * 1024 * 1024;

const CrawlStatus = Type.Object({
  dataPath: Nullable(Type.String()),
  version: Nullable(Type.String()),
  complete: Type.Boolean(),
  done: Type.Integer(),
  expected: Nullable(Type.Integer()),
  blocked: Nullable(Type.String()),
  missing: Type.Array(
    Type.Object({ kind: Type.String(), key: Type.String(), path: Type.String() }),
  ),
});

const DocQuery = Type.Object({
  dataPath: Type.String({ minLength: 1 }),
  kind: Type.String({ minLength: 1 }),
  key: Type.Optional(Type.String()),
});

export const crawlRoutes: FastifyPluginAsyncTypebox<CrawlRouteOptions> = async (
  app,
  { crawl, adminToken },
) => {
  app.addContentTypeParser(
    "application/octet-stream",
    { parseAs: "buffer", bodyLimit: BODY_LIMIT },
    (_request, body, done) => done(null, body),
  );
  app.addHook("onRequest", async (request) => {
    authorize(adminToken, request.headers.authorization);
  });

  const reply200 = { response: { 200: CrawlStatus } };

  app.get("/admin/crawl", { schema: reply200 }, async () => crawl.status());

  app.post("/admin/crawl/versions", { schema: reply200, bodyLimit: BODY_LIMIT }, async ({ body }) =>
    crawl.versions(bufferBody(body)),
  );

  app.put(
    "/admin/crawl/doc",
    { schema: { ...reply200, querystring: DocQuery }, bodyLimit: BODY_LIMIT },
    async ({ query, body }) =>
      crawl.ingest(query.dataPath, query.kind, query.key ?? "", bufferBody(body)),
  );

  app.put(
    "/admin/crawl/absent",
    { schema: { ...reply200, querystring: DocQuery } },
    async ({ query }) => crawl.ingest(query.dataPath, query.kind, query.key ?? "", null),
  );
};

function bufferBody(body: unknown): Buffer {
  if (!Buffer.isBuffer(body) || body.length === 0) {
    throw new BadRequestError("send the document as an application/octet-stream body");
  }
  return body;
}
