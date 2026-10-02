/**
 * SQLite: opening the file, its pragmas, and the schema migrations.
 *
 * better-sqlite3 is synchronous, and that is deliberate: every query here takes microseconds, and a
 * single connection serialises writes the way SQLite wants them anyway. Queries live in `store.ts`.
 */

import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import BetterSqlite3 from "better-sqlite3";

export type Connection = BetterSqlite3.Database;

/**
 * Schema migrations, in order. Each is applied exactly once, tracked by SQLite's own `user_version`,
 * and the position in this list *is* the version, so migrations are only ever appended, never
 * reordered or removed. The files are shared with the Rust version of this service, byte for byte,
 * so a database either one wrote opens in the other.
 */
export const MIGRATIONS: readonly string[] = [
  "0001_init.sql",
  "0002_anvil_rankings.sql",
  "0003_anvil_rankings_history.sql",
].map((file) => readFileSync(new URL(`../../migrations/${file}`, import.meta.url), "utf8"));

export interface OpenOptions {
  /** Called once per migration applied, for the log. */
  onMigrated?: (version: number) => void;
}

/** Opens (creating if needed) the database at `path` and brings its schema up to date. */
export function openDatabase(path: string, { onMigrated }: OpenOptions = {}): Connection {
  const dir = dirname(path);
  if (dir !== "" && dir !== ".") {
    try {
      mkdirSync(dir, { recursive: true });
    } catch (error) {
      throw new Error(`cannot create database directory ${dir}: ${String(error)}`);
    }
    checkWritable(dir);
  }

  const db = new BetterSqlite3(path);
  // WAL lets reads proceed while a write is in progress, and survives a crash mid-write.
  db.pragma("journal_mode = WAL");
  db.pragma("synchronous = NORMAL");
  db.pragma("foreign_keys = ON");
  db.pragma("busy_timeout = 5000");

  try {
    migrate(db, onMigrated);
  } catch (error) {
    db.close();
    throw error;
  }
  return db;
}

export function migrate(db: Connection, onMigrated?: (version: number) => void): void {
  const applied = db.pragma("user_version", { simple: true }) as number;
  if (applied < 0) {
    throw new Error("database reports a negative schema version");
  }
  if (applied > MIGRATIONS.length) {
    throw new Error(
      `database is at schema version ${applied}, newer than this build knows (${MIGRATIONS.length}); ` +
        "it was written by a later deploy and must not be downgraded",
    );
  }

  MIGRATIONS.slice(applied).forEach((sql, offset) => {
    const version = applied + offset + 1;
    db.transaction(() => {
      try {
        db.exec(sql);
      } catch (error) {
        throw new Error(`applying migration ${version}: ${String(error)}`);
      }
      // `user_version` takes no bound parameters; `version` is an index into a constant list, never
      // user input.
      db.pragma(`user_version = ${version}`);
    })();
    onMigrated?.(version);
  });
}

/**
 * Fails early, and legibly, when the database directory cannot be written to.
 *
 * On Railway the cause is a volume mounted root-owned over a directory the service account owned at
 * build time. The entrypoint fixes the ownership; this check is what names the problem if it ever
 * comes back, instead of SQLite's "unable to open database file", which says nothing about why.
 */
function checkWritable(dir: string): void {
  const probe = join(dir, ".write-probe");
  try {
    writeFileSync(probe, "");
  } catch (error) {
    throw new Error(
      `the database directory ${dir} is not writable by uid ${process.getuid?.() ?? "n/a"}: ` +
        `${String(error)}. On Railway this usually means the volume is mounted root-owned while the ` +
        "service runs unprivileged; see service/docker-entrypoint.sh.",
    );
  }
  rmSync(probe, { force: true });
}

/** Seconds since the Unix epoch. Every timestamp column stores this. */
export function nowUnix(): number {
  return Math.floor(Date.now() / 1000);
}
