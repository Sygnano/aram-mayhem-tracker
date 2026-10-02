import { join } from "node:path";
import BetterSqlite3 from "better-sqlite3";
import { describe, expect, it, onTestFinished } from "vitest";
import { tempDatabase, tempDir } from "../test/helpers.ts";
import { MIGRATIONS, migrate, openDatabase } from "./database.ts";
import { Store } from "./store.ts";

function rawConnection(path: string) {
  const conn = new BetterSqlite3(path);
  onTestFinished(() => {
    conn.close();
  });
  return conn;
}

describe("migrations", () => {
  it("apply once and are idempotent", () => {
    const { db } = tempDatabase();

    expect(
      db.pragma("user_version", { simple: true }),
      "user_version tracks the applied count",
    ).toBe(MIGRATIONS.length);

    // Re-running must be a no-op: the tables already exist, so a second CREATE would throw.
    migrate(db);

    const tables = db
      .prepare(
        `SELECT COUNT(*) AS n FROM sqlite_master WHERE type = 'table' AND name IN
         ('versions','upstream_docs','derived','fetch_failures','meta','anvil_rankings')`,
      )
      .get() as { n: number };
    expect(tables.n).toBe(6);
  });

  it("refuse a database from a newer deploy", () => {
    const { db } = tempDatabase();
    db.pragma(`user_version = ${MIGRATIONS.length + 1}`);

    expect(() => migrate(db)).toThrow(/newer than this build knows/);
  });

  it("carry the single saved ranking over into the history", () => {
    const conn = rawConnection(join(tempDir(), "cache.db"));

    // A database as migration 2 left it, holding the one save that design allowed.
    conn.exec(MIGRATIONS[0] ?? "");
    conn.exec(MIGRATIONS[1] ?? "");
    conn.pragma("user_version = 2");
    conn
      .prepare("INSERT INTO anvil_rankings (id, body, saved_at) VALUES (1, 'authored', 42)")
      .run();

    migrate(conn);

    const row = conn
      .prepare("SELECT body, saved_at AS savedAt FROM anvil_rankings ORDER BY id DESC LIMIT 1")
      .get();
    expect(row).toEqual({ body: "authored", savedAt: 42 });
    // And a second row is now allowed, which `CHECK (id = 1)` used to refuse.
    conn.prepare("INSERT INTO anvil_rankings (body, saved_at) VALUES ('next', 43)").run();
  });
});

describe("the store", () => {
  it("opens and reports an empty store", () => {
    const { db, path } = tempDatabase();
    const store = new Store(db);

    store.ping();
    expect(store.latestVersion()).toBeNull();
    expect(store.championsCached("data/none")).toBe(0);

    // Opening the same file again must be a no-op, not a failed re-migration.
    openDatabase(path).close();
  });
});
