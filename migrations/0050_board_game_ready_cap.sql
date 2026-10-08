-- CrowdSync §3 (docs/DEVELOPER_PLATFORM.md): a board waits on a connected game until it says
-- "ready", and the streamer or the game caps the inputs per second forwarded to it.
ALTER TABLE boards
    -- The game session the board waits on, and its 30-second heartbeat (a crashed API can't
    -- leave a board "Starting…" for more than a minute).
    ADD COLUMN game_session text,
    ADD COLUMN game_seen_at timestamptz,
    ADD COLUMN game_ready boolean NOT NULL DEFAULT false,
    ADD COLUMN input_cap smallint CHECK (input_cap BETWEEN 1 AND 100);
