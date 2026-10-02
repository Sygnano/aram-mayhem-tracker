/**
 * `/v1/champion`: everything the app needs about one champion, in one response.
 *
 * The app asks for this each time its champion changes during champ select, which in ARAM is when
 * champ select opens and on every bench swap. It used to send fourteen requests for the same thing:
 * twelve `/v1/pool`, one `/v1/build` and one `/v1/anvil-rankings`. All of it comes from one champion
 * document and the saved rankings, so one response costs the service no more than one of those did.
 *
 * Kept out of the bundle: the champion table and the shard catalogue, which are the same for every
 * champion and fetched once per patch, and the enemy team's damage split, which is not known before
 * the game starts.
 */

import type { FastifyPluginAsyncTypebox } from "@fastify/type-provider-typebox";
import { Type } from "typebox";
import type { Data } from "../data.ts";
import type { Store } from "../db/store.ts";
import { STAGES } from "../domain/augments.ts";
import { SavedRankings, savedRankings } from "./anvils.ts";
import { AugmentInfo, championInfo, poolAugments } from "./augments.ts";
import { Archetypes, knownChampion } from "./champions.ts";
import { ChampionInfo, PatchFields } from "./schemas.ts";
import { currentVersion } from "./status.ts";

export interface BundleRouteOptions {
  store: Store;
  data: Data;
}

/** The rarities an offer can be, lowest first. */
export const RARITIES = ["silver", "gold", "prismatic"] as const;

// -- wire types ------------------------------------------------------------------------------------

const ChampionBundle = Type.Object({
  ...PatchFields,
  champion: ChampionInfo,
  /**
   * One ranked pool per rarity and stage, twelve in all, in the order of `RARITIES` then `STAGES`.
   * Each is what `/v1/pool?rarity=…&stage=…` answers with.
   */
  pools: Type.Array(
    Type.Object({
      rarity: Type.String(),
      stage: Type.Integer(),
      augments: Type.Array(AugmentInfo),
    }),
  ),
  /** The build archetypes the item sets are made from, as `/v1/build` answers. */
  archetypes: Archetypes,
  /**
   * The anvil rankings document reduced to this champion's group: one group, or none when the
   * champion is in no group. Same shape as `/v1/anvil-rankings`.
   */
  anvilRankings: SavedRankings,
});

const ChampionQuery = Type.Object({ champion: Type.Integer() });

// -- handler ---------------------------------------------------------------------------------------

export const bundleRoutes: FastifyPluginAsyncTypebox<BundleRouteOptions> = async (
  app,
  { store, data },
) => {
  app.get(
    "/v1/champion",
    { schema: { querystring: ChampionQuery, response: { 200: ChampionBundle } } },
    async ({ query, log }) => {
      const version = currentVersion(store);
      await knownChampion(data, version.dataPath, query.champion, log);

      const [champion, global, catalogue, archetypes] = await Promise.all([
        data.championAugments(version.dataPath, query.champion),
        data.globalRankings(version.dataPath),
        data.catalogue(version.version),
        data.championBuilds(version.dataPath, query.champion),
      ]);

      const pools = RARITIES.flatMap((rarity) =>
        STAGES.map((stage) => ({
          rarity,
          stage,
          augments: poolAugments(rarity, stage, champion, global, catalogue),
        })),
      );

      const rankings = savedRankings(store);
      return {
        patch: version.version,
        dataDate: version.dataDate,
        champion: championInfo(champion),
        pools,
        archetypes,
        anvilRankings: {
          ...rankings,
          groups: rankings.groups.filter((g) => g.champions.includes(query.champion)),
        },
      };
    },
  );
};
