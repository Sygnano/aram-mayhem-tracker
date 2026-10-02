/** `/v1/champions`, `/v1/build` and `/v1/damage`: champion-level data. */

import type { FastifyPluginAsyncTypebox } from "@fastify/type-provider-typebox";
import type { FastifyBaseLogger } from "fastify";
import { Type } from "typebox";
import type { Data } from "../data.ts";
import type { Store } from "../db/store.ts";
import { roundHalfAway } from "../domain/anvils.ts";
import { type DamageProfile, teamDamage } from "../domain/damage.ts";
import { BadRequestError, NotFoundError, upstreamStatus } from "../errors.ts";
import { splitIds } from "./augments.ts";
import { Nullable, PatchFields } from "./schemas.ts";
import { currentVersion } from "./status.ts";

export interface ChampionRouteOptions {
  store: Store;
  data: Data;
}

// -- wire types ------------------------------------------------------------------------------------

/**
 * One champion's standing in ARAM: Mayhem, for the champ-select overlay. `rank` is aramkit's own, 1
 * is best, and is only meaningful against the `poolSize` on the envelope.
 */
const ChampionRanking = Type.Object({
  id: Type.Integer(),
  rank: Type.Integer(),
  tier: Nullable(Type.String()),
  /** A fraction, as everywhere else in this contract: 0.5778 is 57.78%. */
  winRate: Type.Number(),
  pickRate: Type.Number(),
  sampleCount: Type.Integer(),
});

/**
 * Every champion aramkit ranks, in one response. Unfiltered on purpose: the whole table is about
 * 10 KB, so the app fetches it once per patch and looks champions up locally.
 */
const ChampionsResponse = Type.Object({
  ...PatchFields,
  /** What a `rank` is out of: the number of champions ranked in this build. */
  poolSize: Type.Integer(),
  /** Best rank first, so the response is usable without sorting it. */
  champions: Type.Array(ChampionRanking),
});

const ItemStat = Type.Object({
  id: Type.Integer(),
  winRate: Type.Number(),
  pickRate: Type.Number(),
  sampleCount: Type.Integer(),
});

export const Archetypes = Type.Array(
  Type.Object({
    key: Type.String(),
    rank: Type.Integer(),
    winRate: Type.Number(),
    pickRate: Type.Number(),
    sampleCount: Type.Integer(),
    starters: Type.Array(
      Type.Object({
        items: Type.Array(Type.Integer()),
        winRate: Type.Number(),
        pickRate: Type.Number(),
        sampleCount: Type.Integer(),
      }),
    ),
    boots: Type.Array(ItemStat),
    core: Type.Array(Type.Integer()),
    situational: Type.Array(ItemStat),
  }),
);

const BuildResponse = Type.Object({
  ...PatchFields,
  championId: Type.Integer(),
  archetypes: Archetypes,
});

/** A damage split: fractions that sum to 1. */
const DamageSplit = {
  physical: Type.Number(),
  magic: Type.Number(),
  trueDamage: Type.Number(),
};

/**
 * How the champions asked about deal their damage, one by one and added up as a team. Meant for an
 * enemy team: whether Armor or Magic Resist is worth more against these five.
 */
const DamageResponse = Type.Object({
  ...PatchFields,
  /** In the order asked for, without the ones in `missing`. */
  champions: Type.Array(
    Type.Object({
      id: Type.Integer(),
      ...DamageSplit,
      /** Average damage to champions per game. What `team` weights each champion by. */
      damagePerGame: Type.Number(),
      sampleCount: Type.Integer(),
    }),
  ),
  /**
   * Ids upstream has no data for. Reported rather than failing the request: four champions out of
   * five still say most of what a team deals.
   */
  missing: Type.Array(Type.Integer()),
  /** The listed champions together, each weighted by `damagePerGame`. `null` when there are none. */
  team: Nullable(Type.Object(DamageSplit)),
});

const ChampionQuery = Type.Object({ champion: Type.Integer() });
/** Comma-separated champion ids. */
const DamageQuery = Type.Object({ champions: Type.String() });

// -- handlers --------------------------------------------------------------------------------------

export const championRoutes: FastifyPluginAsyncTypebox<ChampionRouteOptions> = async (
  app,
  { store, data },
) => {
  app.get("/v1/champions", { schema: { response: { 200: ChampionsResponse } } }, async () => {
    const version = currentVersion(store);
    const table = await data.championTable(version.dataPath);

    const champions = Object.values(table.champions)
      .map((c) => ({
        id: c.id,
        rank: c.rank,
        tier: c.tier === "" ? null : c.tier,
        winRate: c.winRate,
        pickRate: c.pickRate,
        sampleCount: c.sampleCount,
      }))
      // Ties are ordered by id, so every client sees the same order.
      .sort((a, b) => a.rank - b.rank || a.id - b.id);

    return {
      patch: version.version,
      dataDate: version.dataDate,
      poolSize: table.poolSize,
      champions,
    };
  });

  app.get(
    "/v1/build",
    { schema: { querystring: ChampionQuery, response: { 200: BuildResponse } } },
    async ({ query, log }) => {
      const version = currentVersion(store);
      await knownChampion(data, version.dataPath, query.champion, log);
      const archetypes = await data.championBuilds(version.dataPath, query.champion);
      return {
        patch: version.version,
        dataDate: version.dataDate,
        championId: query.champion,
        archetypes,
      };
    },
  );

  app.get(
    "/v1/damage",
    { schema: { querystring: DamageQuery, response: { 200: DamageResponse } } },
    async ({ query, log }) => {
      const version = currentVersion(store);
      const ids = parseChampionIds(query.champions);

      // Side by side: a champion nobody has locked in yet costs one upstream fetch, and a cold team
      // is five of them. The upstream client caps how many run at once.
      const ranked = await rankedChampions(data, version.dataPath, log);
      const missing: number[] = [];
      const asked = ids.filter((id) => {
        const known = ranked === null || ranked.has(id);
        if (!known) {
          missing.push(id);
        }
        return known;
      });
      const results = await Promise.allSettled(
        asked.map((id) => data.championDamage(version.dataPath, id)),
      );

      const found = new Map<number, DamageProfile>();
      results.forEach((result, i) => {
        const id = asked[i] as number;
        if (result.status === "fulfilled") {
          if (result.value === null) {
            missing.push(id);
          } else {
            found.set(id, result.value);
          }
        } else if (upstreamStatus(result.reason) === 404) {
          // Upstream has no such champion.
          missing.push(id);
        } else {
          // Anything else (upstream down, a document that does not parse) fails the request, so
          // the app keeps what it had instead of a partial team.
          throw result.reason;
        }
      });
      missing.sort((a, b) => a - b);

      const profiles = ids.flatMap((id) => found.get(id) ?? []);
      const team = teamDamage(profiles);
      return {
        patch: version.version,
        dataDate: version.dataDate,
        champions: profiles.map((p) => ({
          id: p.id,
          ...damageSplit(p),
          damagePerGame: roundHalfAway(p.damagePerGame),
          sampleCount: p.sampleCount,
        })),
        missing,
        team: team === null ? null : damageSplit(team),
      };
    },
  );
};

// -- helpers ---------------------------------------------------------------------------------------

/** Four decimals, like upstream's own rates: 0.8303 is 83.03%. */
function damageSplit(profile: DamageProfile) {
  const round = (v: number) => roundHalfAway(v * 10_000) / 10_000;
  return {
    physical: round(profile.physical),
    magic: round(profile.magic),
    trueDamage: round(profile.trueDamage),
  };
}

export function parseChampionIds(raw: string): number[] {
  const parsed = splitIds(raw);
  if (parsed === null) {
    throw new BadRequestError(`champions must be comma-separated ids, got ${JSON.stringify(raw)}`);
  }
  // A champion asked about twice would count twice in the team.
  const ids = [...new Set(parsed)];
  if (ids.length === 0) {
    throw new BadRequestError("no champion ids given");
  }
  if (ids.some((id) => id <= 0)) {
    throw new BadRequestError("champion ids are positive");
  }
  if (ids.length > 10) {
    // A team is five; ten is both teams.
    throw new BadRequestError(`too many champion ids (${ids.length})`);
  }
  return ids;
}

/**
 * The ids aramkit ranks on this build, or `null` when that table cannot be had.
 *
 * The table is one document per patch and is already in the store for champ select, so this is a
 * SQLite read. It is what stands between an arbitrary integer in a query string and a request to
 * aramkit under our User-Agent.
 */
async function rankedChampions(
  data: Data,
  dataPath: string,
  log: FastifyBaseLogger,
): Promise<Set<number> | null> {
  try {
    const table = await data.championTable(dataPath);
    return new Set(Object.keys(table.champions).map(Number));
  } catch (error) {
    // Not a reason to refuse everybody: a champion's own document may well be cached. The rate
    // limit still bounds what gets through.
    log.warn({ err: error }, "no champion table to validate ids against");
    return null;
  }
}

/** Refuses a champion id aramkit does not rank, before anything is fetched for it. */
export async function knownChampion(
  data: Data,
  dataPath: string,
  id: number,
  log: FastifyBaseLogger,
): Promise<void> {
  const ranked = await rankedChampions(data, dataPath, log);
  if (ranked !== null && !ranked.has(id)) {
    throw new NotFoundError(`no champion with id ${id} in this patch`);
  }
}
