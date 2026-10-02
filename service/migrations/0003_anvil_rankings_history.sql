-- Keeps every save of the anvil rankings instead of only the last one.
--
-- The rankings are the one thing here nothing upstream can rebuild, and 0002 kept a single row that
-- each Save overwrote, so a bad save could only be undone from a log line. Saves are now appended and
-- the newest row is the current document. The table is rebuilt because 0002's `CHECK (id = 1)`
-- cannot be dropped in place; the existing save, if any, becomes the first row of the history.
CREATE TABLE anvil_rankings_history (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    body     TEXT    NOT NULL,  -- the JSON document, as validated on save
    saved_at INTEGER NOT NULL
);

INSERT INTO anvil_rankings_history (body, saved_at)
SELECT body, saved_at FROM anvil_rankings;

DROP TABLE anvil_rankings;
ALTER TABLE anvil_rankings_history RENAME TO anvil_rankings;
