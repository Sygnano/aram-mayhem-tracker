-- aramkit is crawled ahead of time instead of fetched on a request (D-090).
--
-- A data version is served only once every document it needs is held. `crawled_at` says when that
-- happened; NULL means the crawl of that version is still going, and it is not served yet.
ALTER TABLE versions ADD COLUMN crawled_at INTEGER;

-- Documents upstream answered 404 for. Everything under a `data_path` is immutable, so a champion
-- aramkit ranks but has no details for stays that way, and the crawl counts it as done rather than
-- waiting for it forever.
CREATE TABLE absent_docs (
    data_path  TEXT    NOT NULL,
    source     TEXT    NOT NULL,
    kind       TEXT    NOT NULL,
    key        TEXT    NOT NULL,
    checked_at INTEGER NOT NULL,
    PRIMARY KEY (data_path, source, kind, key)
) WITHOUT ROWID;
