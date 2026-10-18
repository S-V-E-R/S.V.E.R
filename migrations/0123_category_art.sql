-- Category cover art and the Steam feed (docs/LIVE_STREAMS.md "Automatically refreshed game catalog").
-- Steam-only games use source ids like steam730; a game Wikidata also knows keeps its Q id.
ALTER TABLE game_catalog DROP CONSTRAINT game_catalog_source_id_check;
ALTER TABLE game_catalog ADD CONSTRAINT game_catalog_source_id_check
    CHECK (source_id ~ '^(Q[1-9][0-9]{0,17}|steam[1-9][0-9]{0,9})$');
ALTER TABLE game_catalog ADD COLUMN steam_app_id bigint UNIQUE;
-- image_key: our stored copy (never a link to the provider). image_manual: staff chose it, so
-- refreshes never replace it. image_checked_at: the last automatic attempt, retried daily.
ALTER TABLE stream_categories
    ADD COLUMN image_key text,
    ADD COLUMN image_manual boolean NOT NULL DEFAULT false,
    ADD COLUMN image_checked_at timestamptz;
CREATE INDEX stream_categories_art ON stream_categories(image_checked_at NULLS FIRST)
    WHERE image_key IS NULL AND NOT image_manual;
-- Steam's app list is walked by app id; a pass only asks for apps changed since the last one.
ALTER TABLE game_catalog_sync
    ADD COLUMN steam_cursor bigint NOT NULL DEFAULT 0,
    ADD COLUMN steam_since bigint NOT NULL DEFAULT 0,
    ADD COLUMN steam_pass bigint NOT NULL DEFAULT 0,
    ADD COLUMN steam_next_run_at timestamptz NOT NULL DEFAULT now(),
    ADD COLUMN steam_failures integer NOT NULL DEFAULT 0,
    ADD COLUMN steam_last_success_at timestamptz;
