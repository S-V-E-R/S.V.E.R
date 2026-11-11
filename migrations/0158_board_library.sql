-- CrowdSync "Creator and developer friendly", Section 1 (docs/CROWDSYNC.md): a library of saved
-- boards per channel, and boards a connected game defines for itself.

-- A saved board is a validated definition; nothing in it is live. `source` is the connection
-- (Studio token ID or app client ID) whose game defined it, so each game keeps one entry.
CREATE TABLE board_library (
    id text PRIMARY KEY,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 40),
    board jsonb NOT NULL,
    source text,
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX board_library_channel ON board_library (channel_id);
CREATE UNIQUE INDEX board_library_source ON board_library (channel_id, source) WHERE source IS NOT NULL;

-- "May set my board": the streamer's opt-in per connection, off by default.
ALTER TABLE integration_tokens ADD COLUMN may_define boolean NOT NULL DEFAULT false;
ALTER TABLE oauth_grants ADD COLUMN may_define boolean NOT NULL DEFAULT false;

-- While a game's board is live: which connection set it (and its name, for Studio), the gateway
-- session that did (it checks in every 30 seconds), and the streamer's own published board, saved
-- aside to come back when the game leaves. All NULL when the live board is the streamer's.
ALTER TABLE boards
    ADD COLUMN set_by text,
    ADD COLUMN set_by_app boolean NOT NULL DEFAULT false,
    ADD COLUMN set_by_name text,
    ADD COLUMN set_session text,
    ADD COLUMN set_seen_at timestamptz,
    ADD COLUMN set_aside jsonb;
