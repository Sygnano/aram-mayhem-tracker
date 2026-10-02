/**
 * Response schemas shared between routes. A response schema is the published contract the desktop
 * app mirrors (`crates/aramkit-client/src/types.rs`): Fastify serialises with it, so a field it does
 * not list is never sent.
 */

import { type TSchema, Type } from "typebox";

export const Nullable = <T extends TSchema>(schema: T) => Type.Union([schema, Type.Null()]);

export const ChampionInfo = Type.Object({
  id: Type.Integer(),
  winRate: Type.Number(),
  pickRate: Type.Number(),
  sampleCount: Type.Integer(),
  tier: Nullable(Type.String()),
});

/** Every response about data carries the patch it is about. */
export const PatchFields = {
  patch: Type.String(),
  dataDate: Type.String(),
};
