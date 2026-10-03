-- Module 3 ingest/control state. No legacy keys or broadcasts are imported.
CREATE TABLE stream_categories (
    id text PRIMARY KEY,
    name text NOT NULL UNIQUE,
    genre text NOT NULL,
    active boolean NOT NULL DEFAULT true
);
-- Small first-party catalog; no faction ownership or ranking is assigned here.
INSERT INTO stream_categories(id,name,genre) VALUES
    ('minecraft','Minecraft','sandbox'),
    ('fortnite','Fortnite','battle_royale'),
    ('counter-strike-2','Counter-Strike 2','fps'),
    ('valorant','VALORANT','fps'),
    ('league-of-legends','League of Legends','moba'),
    ('art','Art','art'),
    ('coding','Coding','education_coding'),
    ('crafting-making','Crafting and making','crafting_making');

CREATE TABLE stream_settings (
    owner_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 140),
    category_id text REFERENCES stream_categories(id),
    revision bigint NOT NULL DEFAULT 0,
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE stream_credentials (
    owner_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    public_id text NOT NULL UNIQUE,
    generation bigint NOT NULL DEFAULT 1,
    secret_hash text,
    secret_cipher text,
    created_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz,
    CHECK ((revoked_at IS NULL AND secret_hash IS NOT NULL AND secret_cipher IS NOT NULL)
        OR (revoked_at IS NOT NULL AND secret_hash IS NULL AND secret_cipher IS NULL))
);
CREATE TABLE broadcasts (
    id text PRIMARY KEY,
    owner_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    public_id text NOT NULL,
    generation bigint NOT NULL,
    state text NOT NULL CHECK (state IN ('STARTING','LIVE','RECONNECTING','ENDED')),
    server_id text NOT NULL,
    service_id text NOT NULL,
    client_id text NOT NULL,
    started_at timestamptz NOT NULL,
    publisher_started_at timestamptz NOT NULL,
    startup_deadline timestamptz NOT NULL,
    reconnect_deadline timestamptz,
    ended_at timestamptz,
    end_reason text,
    checked_at timestamptz,
    observed_at timestamptz,
    recv_bytes bigint,
    health jsonb NOT NULL DEFAULT '{}',
    CHECK ((state='ENDED') = (ended_at IS NOT NULL)),
    CHECK ((state='RECONNECTING') = (reconnect_deadline IS NOT NULL))
);
CREATE UNIQUE INDEX one_open_broadcast ON broadcasts(owner_id) WHERE state<>'ENDED';
CREATE INDEX broadcasts_poll ON broadcasts(checked_at) WHERE state<>'ENDED';
-- Durable replay tombstones also handle unpublish arriving before publish.
CREATE TABLE stream_publishers (
    server_id text NOT NULL,
    service_id text NOT NULL,
    client_id text NOT NULL,
    owner_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    broadcast_id text REFERENCES broadcasts(id) ON DELETE CASCADE,
    public_id text NOT NULL,
    retired_at timestamptz,
    PRIMARY KEY(server_id,service_id,client_id)
);
CREATE TABLE stream_stop_jobs (
    server_id text NOT NULL,
    service_id text NOT NULL,
    client_id text NOT NULL,
    public_id text NOT NULL,
    owner_id text REFERENCES users(id) ON DELETE SET NULL,
    available_at timestamptz NOT NULL DEFAULT now(),
    attempts integer NOT NULL DEFAULT 0,
    PRIMARY KEY(server_id,service_id,client_id)
);
CREATE INDEX stream_stop_jobs_due ON stream_stop_jobs(available_at);
