-- A durable local mirror. Provider outages never remove choices or block publishing.
CREATE TABLE game_catalog (
    source_id text PRIMARY KEY CHECK (source_id ~ '^Q[1-9][0-9]{0,17}$'),
    category_id text REFERENCES stream_categories(id),
    name text NOT NULL,
    aliases text[] NOT NULL DEFAULT '{}',
    genres text[] NOT NULL DEFAULT '{}',
    description text NOT NULL DEFAULT '',
    suggested_genre text REFERENCES faction_genres(id),
    reviewed boolean NOT NULL DEFAULT false,
    refreshed_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX game_catalog_category ON game_catalog(category_id);
CREATE INDEX game_catalog_name ON game_catalog(lower(name));
CREATE INDEX game_catalog_pending ON game_catalog(name) WHERE category_id IS NULL AND NOT reviewed;
CREATE TABLE game_catalog_sync (
    id boolean PRIMARY KEY DEFAULT true CHECK (id),
    cursor text NOT NULL DEFAULT '',
    recent boolean NOT NULL DEFAULT true,
    next_run_at timestamptz NOT NULL DEFAULT now(),
    lease_until timestamptz,
    lease_token text,
    last_success_at timestamptz,
    last_complete_at timestamptz,
    failures integer NOT NULL DEFAULT 0
);
INSERT INTO game_catalog_sync DEFAULT VALUES;
