/**
 * The stat anvil rankings document.
 *
 * It is the one thing in this service that exists nowhere else, so a save is validated in full
 * before it is stored, and every problem is reported at once so the editor can show them together.
 */

import { type AnvilCatalogue, hasShard, TIERS, type Tier } from "./anvils.ts";

/**
 * The only document version there is. Bumped if the shape ever changes, so an old editor tab cannot
 * overwrite a newer document with one it misunderstands.
 */
export const DOC_VERSION = 1;
const MAX_GROUPS = 200;
const MAX_GROUP_NAME = 60;

/**
 * Each tier is an ordered list of tie buckets: bucket `i` holds the shards ranked `i + 1`, so dense
 * ranks (`1, 2, 2, 3`) fall out of the shape and nothing is ever renumbered. A shard in no bucket is
 * unranked.
 */
export type Tiers = Record<Tier, string[][]>;

export interface Group {
  name: string;
  /** Riot champion ids. A champion is in at most one group; one in none gets no labels. */
  champions: number[];
  tiers: Tiers;
}

export interface RankingDoc {
  version: number;
  groups: Group[];
}

export function emptyDoc(): RankingDoc {
  return { version: DOC_VERSION, groups: [] };
}

/** Input as it arrives, with the parts the document lets a client leave out. */
export interface RankingInput {
  version: number;
  groups: { name: string; champions?: number[]; tiers?: Partial<Tiers> }[];
}

/**
 * The document with every default filled in and nothing else kept. Unknown fields, such as the
 * `savedAt` the editor sends back with what it loaded, are dropped.
 */
export function normaliseDoc(input: RankingInput): RankingDoc {
  return {
    version: input.version,
    groups: input.groups.map((g) => ({
      name: g.name,
      champions: [...(g.champions ?? [])],
      tiers: {
        silver: g.tiers?.silver ?? [],
        gold: g.tiers?.gold ?? [],
        prismatic: g.tiers?.prismatic ?? [],
      },
    })),
  };
}

/** Quotes a name the way the messages always have. */
const q = (s: string) => JSON.stringify(s);

/** Every problem with a document, not just the first. */
export function validate(doc: RankingDoc, catalogue: AnvilCatalogue): string[] {
  const problems: string[] = [];

  if (doc.version !== DOC_VERSION) {
    problems.push(`document version ${doc.version} is not ${DOC_VERSION}; reload the editor`);
  }
  if (doc.groups.length > MAX_GROUPS) {
    problems.push(`more than ${MAX_GROUPS} groups`);
  }

  const names = new Set<string>();
  const owner = new Map<number, string>();
  for (const group of doc.groups) {
    const name = group.name.trim();
    if (name === "") {
      problems.push("a group has no name");
    } else if ([...name].length > MAX_GROUP_NAME) {
      problems.push(`group name ${q(name)} is longer than ${MAX_GROUP_NAME} characters`);
    } else if (names.has(name.toLowerCase())) {
      problems.push(`two groups are named ${q(name)}`);
    } else {
      names.add(name.toLowerCase());
    }

    for (const champion of group.champions) {
      if (champion <= 0) {
        problems.push(`group ${q(name)} lists champion id ${champion}`);
        continue;
      }
      const other = owner.get(champion);
      owner.set(champion, name);
      if (other === name) {
        problems.push(`group ${q(name)} lists champion ${champion} twice`);
      } else if (other !== undefined) {
        problems.push(`champion ${champion} is in both ${q(other)} and ${q(name)}`);
      }
    }

    for (const tier of TIERS) {
      const seen = new Set<string>();
      group.tiers[tier].forEach((bucket, rank) => {
        if (bucket.length === 0) {
          problems.push(`group ${q(name)}, ${tier}: rank ${rank + 1} is empty`);
        }
        for (const id of bucket) {
          if (!hasShard(catalogue, tier, id)) {
            problems.push(`group ${q(name)}, ${tier}: ${id} is not a ${tier} shard this patch`);
          }
          if (seen.has(id)) {
            problems.push(`group ${q(name)}, ${tier}: ${id} is ranked twice`);
          }
          seen.add(id);
        }
      });
    }
  }
  return problems;
}
