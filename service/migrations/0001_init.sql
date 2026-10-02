-- Phase 3 schema.
--
-- Two layers on purpose: `upstream_docs` holds raw aramkit / CommunityDragon bodies exactly as
-- fetched and is only ever inserted into, while `derived` holds parsed payloads and can be dropped
-- and rebuilt from the raw layer without refetching anything.

-- One row per aramkit build we have ever seen. `data_path` is content-addressed, so a rebuild of the
-- same patch arrives as a new row rather than an update.
CREATE TABLE versions (
    version       TEXT    NOT NULL,          -- "16.19"
    data_path     TEXT    PRIMARY KEY,       -- "data/16.19-20260927-05de64d31ef1"
    resource_path TEXT    NOT NULL,
    data_date     TEXT    NOT NULL,          -- "2026-09-27"
    build_time_ms INTEGER NOT NULL,
    all_matches   INTEGER NOT NULL,
    high_matches  INTEGER NOT NULL,
    is_latest     INTEGER NOT NULL DEFAULT 0,
    first_seen_at INTEGER NOT NULL
);

CREATE INDEX versions_latest ON versions (is_latest) WHERE is_latest = 1;

-- Raw upstream documents, gzipped. Immutable: everything under a `data_path` is served by aramkit as
-- `immutable, max-age=1y`, so a row here is never revalidated.
CREATE TABLE upstream_docs (
    data_path  TEXT    NOT NULL,  -- '' for CommunityDragon documents, which are patch-pinned differently
    source     TEXT    NOT NULL,  -- 'aramkit' | 'cdragon'
    kind       TEXT    NOT NULL,  -- 'champion-details' | 'augment-rankings' | 'cherry-augments' | 'augment-lists'
    key        TEXT    NOT NULL,  -- champion id as text, or '' for singletons
    body       BLOB    NOT NULL,  -- gzip(json)
    etag       TEXT,
    fetched_at INTEGER NOT NULL,
    PRIMARY KEY (data_path, source, kind, key)
) WITHOUT ROWID;

-- Parsed, slim payloads. Safe to delete: rebuilt from `upstream_docs`. `schema_ver` is bumped when
-- the parser changes, so a deploy starts writing new rows and ignores the old ones.
CREATE TABLE derived (
    data_path  TEXT    NOT NULL,
    kind       TEXT    NOT NULL,  -- 'champion-augments' | 'champion-builds' | 'augment-rankings'
    key        TEXT    NOT NULL,
    schema_ver INTEGER NOT NULL,
    body       BLOB    NOT NULL,  -- gzip(json)
    built_at   INTEGER NOT NULL,
    PRIMARY KEY (data_path, kind, key, schema_ver)
) WITHOUT ROWID;

-- Upstream failures, so a champion that 404s is not refetched in a hot loop.
CREATE TABLE fetch_failures (
    data_path TEXT    NOT NULL,
    source    TEXT    NOT NULL,
    kind      TEXT    NOT NULL,
    key       TEXT    NOT NULL,
    status    INTEGER,
    message   TEXT,
    failed_at INTEGER NOT NULL,
    attempts  INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (data_path, source, kind, key)
) WITHOUT ROWID;

CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
