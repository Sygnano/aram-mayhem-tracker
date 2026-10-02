import { describe, expect, it } from "vitest";
import { tempDatabase } from "../test/helpers.ts";
import { aramkitKey, cdragonKey, HISTORY_KEPT, Store, type VersionEntry } from "./store.ts";

function store() {
  return new Store(tempDatabase().db);
}

describe("the anvil rankings history", () => {
  it("makes a save current and keeps the ones before", () => {
    const s = store();

    expect(s.anvilRankings()).toBeNull();
    expect(s.saveAnvilRankings("one", 1)).toBe(1);
    expect(s.saveAnvilRankings("two", 2)).toBe(2);
    expect(s.anvilRankings()).toEqual({ body: "two", savedAt: 2 });

    const history = s.db.prepare("SELECT body FROM anvil_rankings ORDER BY id").pluck().all();
    expect(history).toEqual(["one", "two"]);

    for (let n = 0; n < HISTORY_KEPT; n++) {
      s.saveAnvilRankings(`more-${n}`, 3);
    }
    expect(s.saveAnvilRankings("last", 4)).toBe(HISTORY_KEPT);
    expect(s.anvilRankings()).toEqual({ body: "last", savedAt: 4 });
  });
});

describe("the document cache", () => {
  it("round-trips a body through gzip", () => {
    const s = store();
    const key = aramkitKey("data/x", "champion-details", "157");
    expect(s.getDoc(key)).toBeNull();

    s.putDoc(key, Buffer.from('{"a":1}'), '"etag"');
    expect(s.getDoc(key)?.toString()).toBe('{"a":1}');
    const stored = s.db.prepare("SELECT body FROM upstream_docs").pluck().get() as Buffer;
    expect([stored[0], stored[1]], "the gzip magic number").toEqual([0x1f, 0x8b]);
  });
});

describe("versions", () => {
  const version = (v: string, dataPath: string): VersionEntry => ({
    version: v,
    dataPath,
    resourcePath: "",
    dataDate: "2026-09-27",
    buildTimeUnixMs: 1,
    allMatches: 1,
    highMatches: 0,
  });

  it("mark the latest and prune what upstream no longer lists", () => {
    const s = store();
    s.recordVersions([version("16.18", "data/old"), version("16.19", "data/new")], "16.19");
    expect(s.latestVersion()?.dataPath).toBe("data/new");

    s.putDoc(aramkitKey("data/old", "champion-details", "157"), Buffer.from("{}"), null);
    s.putDoc(cdragonKey("cherry-augments", "16.18"), Buffer.from("[]"), null);

    s.recordVersions([version("16.19", "data/new"), version("16.20", "data/newer")], "16.20");
    expect(s.pruneOldPatches(["data/new", "data/newer"])).toBe(1);
    expect(s.latestVersion()?.version).toBe("16.20");
    // CommunityDragon documents are pinned by patch, not by an aramkit build, and are kept.
    expect(s.getDoc(cdragonKey("cherry-augments", "16.18"))).not.toBeNull();
  });
});
