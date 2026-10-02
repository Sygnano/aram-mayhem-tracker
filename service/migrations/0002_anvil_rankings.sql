-- Stat anvil rankings.
--
-- Unlike every other table, this one is NOT a cache: it holds what the project owner authored in
-- /admin, and nothing upstream can rebuild it. Exactly one save exists; every Save replaces it.
CREATE TABLE anvil_rankings (
    id       INTEGER PRIMARY KEY CHECK (id = 1),
    body     TEXT    NOT NULL,  -- the JSON document, as validated on save
    saved_at INTEGER NOT NULL
);
