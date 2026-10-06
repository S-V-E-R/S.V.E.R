-- Module 7 CrowdSync, part 2 (docs/CROWDSYNC.md "Outputs", "Game SDK"): scoped tokens for the
-- S.V.E.R bridge (OBS on the streamer's PC) and for games, which connect only through the API's
-- integration gateway, never to the database or internal services. 0037 is Module 8's.

CREATE TABLE integration_tokens (
    id text PRIMARY KEY,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- 'bridge' receives board events; 'game' also sends board state back.
    kind text NOT NULL CHECK (kind IN ('bridge', 'game')),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 40),
    token_hash text NOT NULL UNIQUE,
    created_at timestamptz NOT NULL DEFAULT now(),
    last_used_at timestamptz,
    revoked_at timestamptz
);
CREATE INDEX integration_tokens_channel ON integration_tokens (channel_id) WHERE revoked_at IS NULL;

-- What a connected game set on the published board: per-control label overrides and availability
-- ({"control-id": {"label": "...", "disabled": true}}). Cleared when a new version is published.
ALTER TABLE boards ADD COLUMN live_state jsonb NOT NULL DEFAULT '{}';
