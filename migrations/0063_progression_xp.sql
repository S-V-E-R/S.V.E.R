-- Progression, part 1 (docs/PROGRESSION.md): XP per user, UTC day and source (caps apply per row;
-- the total is the sum), and Scout bonuses for early viewers of new channels.
CREATE TABLE xp_days (
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    day date NOT NULL,
    source text NOT NULL CHECK (source IN ('watch', 'chat', 'scout', 'orders')),
    xp integer NOT NULL CHECK (xp >= 0),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, day, source)
);
CREATE TABLE scout_awards (
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    broadcast_id text NOT NULL REFERENCES broadcasts(id) ON DELETE CASCADE,
    at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, broadcast_id)
);
CREATE INDEX scout_awards_recent ON scout_awards (user_id, at);
CREATE INDEX scout_awards_broadcast ON scout_awards (broadcast_id);
