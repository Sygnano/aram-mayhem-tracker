/**
 * The stat anvil catalogue.
 *
 * Mayhem's shards are not in `cherry-augments.json` or `items.json`. They are `AnvilData` objects in
 * the ARAM map's bin, `game/data/maps/shipping/map12/map12.bin.json`, each pointing at a spell whose
 * `DataValues` are the shard's fixed stats. Names are keys into the per-locale string table.
 *
 * Two shards in the bin are not offered in Mayhem (the wiki and the project owner agree): Two Random
 * Shards and Shardholder Value. They are excluded by id.
 */

/** In the bin but never offered in Mayhem. */
const EXCLUDED = new Set([
  "ARAM_StatAnvil_Random",
  "ARAM_GoldStatAnvil_Random",
  "ARAM_StatAnvil_Pristine",
]);

/**
 * Locales CommunityDragon publishes game string tables for. A request for anything else is refused
 * before it can cost a 31 MB fetch.
 */
export const LOCALES: readonly string[] = [
  "ar_ae",
  "cs_cz",
  "de_de",
  "el_gr",
  "en_au",
  "en_gb",
  "en_ph",
  "en_sg",
  "en_us",
  "es_ar",
  "es_es",
  "es_mx",
  "fr_fr",
  "hu_hu",
  "id_id",
  "it_it",
  "ja_jp",
  "ko_kr",
  "pl_pl",
  "pt_br",
  "ro_ro",
  "ru_ru",
  "th_th",
  "tr_tr",
  "vi_vn",
  "zh_cn",
  "zh_my",
  "zh_tw",
];

/** In this order everywhere: silver, then gold, then prismatic. */
export const TIERS = ["silver", "gold", "prismatic"] as const;
export type Tier = (typeof TIERS)[number];

/** `AnvilTypes` codes as the bin uses them, checked against all 37 entries on 16.19. */
const TIER_CODES: ReadonlyMap<number, Tier> = new Map([
  [0, "silver"],
  [8, "gold"],
  [9, "prismatic"],
]);

/** One shard in one tier, as clients see it. */
export interface Shard {
  /** `AugmentNameId`, e.g. `ARAM_GoldStatAnvil_MR`. What rankings are stored against. */
  id: string;
  tier: Tier;
  /**
   * The stats the shard grants, as their `DataValues` names joined with `+` (`MinAD+MinAP`).
   *
   * Shards with the same kind are the same stat in different tiers. That is what lets a client read
   * a name, which never gives the tier, and then settle the tier from the numbers.
   */
  kind: string;
  /** Display name in the requested locale. */
  name: string;
  /**
   * The numbers printed on the card, in `DataValues` order. Fractions are converted to the
   * percentages the card prints: 0.25 becomes 25, 0.175 becomes 17.5.
   */
  values: number[];
  iconUrl: string;
}

export interface AnvilCatalogue {
  patch: string;
  locale: string;
  /** Silver, then gold, then prismatic; within a tier, by id. Stable so clients can diff it. */
  shards: Shard[];
}

/** A shard as read from the bin, before names are attached. */
export interface RawShard {
  id: string;
  nameKey: string;
  tiers: Tier[];
  kind: string;
  values: number[];
  iconPath: string;
}

export function hasShard(catalogue: AnvilCatalogue, tier: Tier, id: string): boolean {
  return catalogue.shards.some((s) => s.tier === tier && s.id === id);
}

type JsonObject = Record<string, unknown>;

const isObject = (v: unknown): v is JsonObject =>
  typeof v === "object" && v !== null && !Array.isArray(v);

/** Byte order on ASCII ids, as Rust's `String` ordering was. */
const compareIds = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

/** Pulls every Mayhem shard out of a parsed `map12.bin.json`. */
export function shardsFromMap(doc: unknown): RawShard[] {
  if (!isObject(doc)) {
    throw new Error("parsing map12.bin.json: not an object");
  }

  const out: RawShard[] = [];
  for (const object of Object.values(doc)) {
    if (!isObject(object) || object.__type !== "AnvilData") {
      continue;
    }
    const field = (name: string) =>
      typeof object[name] === "string" ? (object[name] as string) : "";
    const id = field("AugmentNameId");
    if (id === "" || EXCLUDED.has(id)) {
      continue;
    }

    const codes = Array.isArray(object.AnvilTypes) ? object.AnvilTypes : [];
    const found = new Set(codes.filter(Number.isInteger).map((c) => TIER_CODES.get(c as number)));
    const tiers = TIERS.filter((t) => found.has(t));
    if (tiers.length === 0) {
      throw new Error(`shard ${id} has no known AnvilTypes`);
    }

    const spell = doc[field("RootSpell")];
    if (spell === undefined) {
      throw new Error(`shard ${id} points at a RootSpell the bin does not have`);
    }
    const mSpell = isObject(spell) ? spell.mSpell : undefined;
    const dataValues =
      isObject(mSpell) && Array.isArray(mSpell.DataValues) ? mSpell.DataValues : [];
    const names: string[] = [];
    const values: number[] = [];
    for (const dv of dataValues) {
      if (!isObject(dv) || typeof dv.name !== "string" || !Array.isArray(dv.values)) {
        continue;
      }
      // Every entry is a 7-long array of one repeated value; the stats are fixed.
      const first: unknown = dv.values[0];
      if (typeof first === "number") {
        names.push(dv.name);
        values.push(displayValue(first));
      }
    }
    if (names.length === 0) {
      throw new Error(`shard ${id} has no DataValues`);
    }

    out.push({
      id,
      nameKey: field("NameTra").toLowerCase(),
      tiers,
      kind: names.join("+"),
      values,
      iconPath: field("AugmentSmallIconPath"),
    });
  }
  return out.sort((a, b) => compareIds(a.id, b.id));
}

/** Rounds half away from zero, as Rust's `f64::round` does. `Math.round` rounds half up. */
export function roundHalfAway(value: number): number {
  return Math.sign(value) * Math.round(Math.abs(value));
}

/**
 * The number the card prints for a `DataValues` entry. Fractions are percentages: the Move Speed
 * card prints `15.0%` for 0.15 and `-10%` for a `SizeMod` of -0.1.
 */
function displayValue(value: number): number {
  const shown = Math.abs(value) < 1 ? value * 100 : value;
  // 0.175 * 100 is 17.499999999999996.
  return roundHalfAway(shown * 10) / 10;
}

/** Reads only `wanted` keys out of a parsed string table's `entries`. */
export function stringsFromTable(table: unknown, wanted: ReadonlySet<string>): Map<string, string> {
  if (!isObject(table)) {
    throw new Error("parsing the string table: not an object");
  }
  const entries = isObject(table.entries) ? table.entries : {};
  const found = new Map<string, string>();
  for (const key of wanted) {
    const value = entries[key];
    // Values are strings in every table seen; anything else is skipped, not fatal.
    if (typeof value === "string") {
      found.set(key, value);
    }
  }
  return found;
}

/** Joins shards and names into the catalogue a client gets. */
export function catalogue(
  patch: string,
  locale: string,
  cdragonBase: string,
  raw: readonly RawShard[],
  names: ReadonlyMap<string, string>,
): AnvilCatalogue {
  const shards: Shard[] = raw.flatMap((r) =>
    r.tiers.map((tier) => {
      const name = cleanName(names.get(r.nameKey) ?? "");
      return {
        id: r.id,
        tier,
        kind: r.kind,
        name: name === "" ? r.id : name,
        values: [...r.values],
        iconUrl: iconUrl(cdragonBase, patch, r.iconPath),
      };
    }),
  );
  shards.sort((a, b) => TIERS.indexOf(a.tier) - TIERS.indexOf(b.tier) || compareIds(a.id, b.id));
  return { patch, locale, shards };
}

/** String table names carry markup now and then: `Armor Penetration Shard<br>` on 16.19. */
function cleanName(raw: string): string {
  let out = "";
  let inTag = false;
  for (const c of raw) {
    if (c === "<") {
      inTag = true;
    } else if (c === ">" && inTag) {
      inTag = false;
    } else if (!inTag) {
      out += c;
    }
  }
  return out.trim();
}

/** `assets/ux/cherry/augments/statanvil/gold_mr.tex` → the PNG CommunityDragon converts it to. */
function iconUrl(base: string, patch: string, path: string): string {
  if (path === "") {
    return "";
  }
  const tail = path
    .replace(/^\/+/, "")
    .toLowerCase()
    .replace(/\.(tex|dds)$/, "");
  return `${base}/${patch}/game/${tail}.png`;
}
