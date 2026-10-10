-- Restreaming (docs/LINKED_CHAT.md "Restreaming"): up to 3 destinations per channel. Keys are
-- sealed by the API and never returned; status is written by the relay supervisor.
CREATE TABLE restream_destinations (
    id text PRIMARY KEY,
    owner_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    platform text NOT NULL CHECK (platform IN ('twitch', 'youtube', 'kick', 'custom')),
    label text NOT NULL DEFAULT '',
    server text NOT NULL,
    key_sealed text NOT NULL,
    enabled boolean NOT NULL DEFAULT true,
    status text NOT NULL DEFAULT 'idle' CHECK (status IN ('idle', 'starting', 'live', 'reconnecting', 'rejected', 'waiting')),
    detail text,
    status_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX restream_destinations_owner ON restream_destinations (owner_id);
