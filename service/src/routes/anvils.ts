/**
 * Stat anvil endpoints.
 *
 * - `GET /v1/anvils?locale=`: the shard catalogue.
 * - `GET /v1/anvil-rankings`: the current rankings document. The editor loads it; the app fetches it
 *   and picks its champion's group locally.
 * - `PUT /v1/anvil-rankings`: the editor's Save. Needs `ADMIN_TOKEN`, becomes the current document.
 * - `GET /admin`: the editor itself.
 *
 * A save is appended rather than written over the one before: the `anvil_rankings` table is the
 * history, and its newest row is the current document.
 */

import { timingSafeEqual } from "node:crypto";
import { readFileSync } from "node:fs";
import type { FastifyPluginAsyncTypebox } from "@fastify/type-provider-typebox";
import { Type } from "typebox";
import type { Data } from "../data.ts";
import { nowUnix } from "../db/database.ts";
import type { Store } from "../db/store.ts";
import { LOCALES } from "../domain/anvils.ts";
import { emptyDoc, normaliseDoc, type RankingDoc, validate } from "../domain/rankings.ts";
import { BadRequestError, ForbiddenError, UnauthorizedError } from "../errors.ts";
import { Nullable } from "./schemas.ts";
import { currentVersion } from "./status.ts";

export interface AnvilRouteOptions {
  store: Store;
  data: Data;
  adminToken: string | null;
}

/**
 * The editor page. Resolved from the source tree whether this file runs from `src/routes/` or from
 * `dist/routes/`; the image copies `src/admin.html` for that reason.
 */
const ADMIN_PAGE = readFileSync(new URL("../../src/admin.html", import.meta.url), "utf8");

// -- wire types ------------------------------------------------------------------------------------

const TierName = Type.Union([
  Type.Literal("silver"),
  Type.Literal("gold"),
  Type.Literal("prismatic"),
]);

const AnvilCatalogue = Type.Object({
  patch: Type.String(),
  locale: Type.String(),
  shards: Type.Array(
    Type.Object({
      id: Type.String(),
      tier: TierName,
      kind: Type.String(),
      name: Type.String(),
      values: Type.Array(Type.Number()),
      iconUrl: Type.String(),
    }),
  ),
});

const Buckets = Type.Array(Type.Array(Type.String()));

/** What the editor sends. Fields a client may leave out are optional; unknown ones are ignored. */
const RankingInput = Type.Object({
  version: Type.Integer({ minimum: 0 }),
  groups: Type.Array(
    Type.Object({
      name: Type.String(),
      champions: Type.Optional(Type.Array(Type.Integer())),
      tiers: Type.Optional(
        Type.Object({
          silver: Type.Optional(Buckets),
          gold: Type.Optional(Buckets),
          prismatic: Type.Optional(Buckets),
        }),
      ),
    }),
  ),
});

/** The document plus when it was saved, `null` before the first save. */
export const SavedRankings = Type.Object({
  savedAt: Nullable(Type.Integer()),
  version: Type.Integer(),
  groups: Type.Array(
    Type.Object({
      name: Type.String(),
      champions: Type.Array(Type.Integer()),
      tiers: Type.Object({ silver: Buckets, gold: Buckets, prismatic: Buckets }),
    }),
  ),
});

const CatalogueQuery = Type.Object({ locale: Type.Optional(Type.String()) });

// -- handlers --------------------------------------------------------------------------------------

/** The current rankings document and when it was saved; an empty one before the first save. */
export function savedRankings(store: Store): { savedAt: number | null } & RankingDoc {
  const saved = store.anvilRankings();
  if (saved === null) {
    return { savedAt: null, ...emptyDoc() };
  }
  let doc: RankingDoc;
  try {
    doc = normaliseDoc(JSON.parse(saved.body));
  } catch (error) {
    throw new Error(`the saved anvil rankings do not parse: ${String(error)}`, { cause: error });
  }
  return { savedAt: saved.savedAt, ...doc };
}

/** The data endpoints, which sit behind the rate limit. */
export const anvilRoutes: FastifyPluginAsyncTypebox<AnvilRouteOptions> = async (
  app,
  { store, data, adminToken },
) => {
  app.get(
    "/v1/anvils",
    { schema: { querystring: CatalogueQuery, response: { 200: AnvilCatalogue } } },
    async ({ query }) => {
      const locale = (query.locale ?? "en_us").trim().toLowerCase();
      if (!LOCALES.includes(locale)) {
        throw new BadRequestError(
          `unknown locale ${JSON.stringify(locale)}; expected one of ${LOCALES.join(", ")}`,
        );
      }
      const { version: patch } = currentVersion(store);
      return data.anvilCatalogue(patch, locale);
    },
  );

  app.get("/v1/anvil-rankings", { schema: { response: { 200: SavedRankings } } }, async () =>
    savedRankings(store),
  );

  app.put(
    "/v1/anvil-rankings",
    { schema: { body: RankingInput, response: { 200: SavedRankings } } },
    async ({ headers, body, log }) => {
      authorize(adminToken, headers.authorization);

      const { version: patch } = currentVersion(store);
      const catalogue = await data.anvilCatalogue(patch, "en_us");
      const doc = normaliseDoc(body);
      const problems = validate(doc, catalogue);
      if (problems.length > 0) {
        throw new BadRequestError(
          `not saved, the previous save is unchanged: ${problems.join("; ")}`,
        );
      }

      for (const group of doc.groups) {
        group.name = group.name.trim();
      }
      const savedAt = nowUnix();
      const kept = store.saveAnvilRankings(JSON.stringify(doc), savedAt);
      // Earlier saves stay in `anvil_rankings`, so a bad one is undone by re-saving an older row.
      log.info({ groups: doc.groups.length, history: kept }, "anvil rankings saved");

      return { savedAt, ...doc };
    },
  );
};

/**
 * The editor page, outside the rate limit. It is the one HTML page here and the one place a token is
 * typed, so it may not be framed by another site.
 */
export const adminRoute: FastifyPluginAsyncTypebox = async (app) => {
  app.get("/admin", async (_request, reply) => {
    return reply
      .header("x-frame-options", "DENY")
      .header(
        "content-security-policy",
        "frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
      )
      .header("referrer-policy", "no-referrer")
      .type("text/html; charset=utf-8")
      .send(ADMIN_PAGE);
  });
};

/**
 * Saving needs `Authorization: Bearer <ADMIN_TOKEN>`. With no token configured, saving is off
 * altogether: a deploy that forgets the variable is locked, not open.
 */
export function authorize(expected: string | null, header: string | undefined): void {
  if (expected === null || expected === "") {
    throw new ForbiddenError("saving is disabled: ADMIN_TOKEN is not set on the service");
  }
  const given = header?.startsWith("Bearer ") ? header.slice("Bearer ".length) : "";
  if (!constantTimeEqual(given, expected)) {
    throw new UnauthorizedError("wrong or missing admin token");
  }
}

/**
 * Compares without an early exit, so response time says nothing about how much of a guess was right.
 * The length is not secret.
 */
function constantTimeEqual(a: string, b: string): boolean {
  const [x, y] = [Buffer.from(a, "utf8"), Buffer.from(b, "utf8")];
  return x.length === y.length && timingSafeEqual(x, y);
}
