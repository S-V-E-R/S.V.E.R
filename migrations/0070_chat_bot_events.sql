-- Chat commands and the channel bot, part 2 (docs/COMMUNITY.md): AutoMod and its ladder, bot event
-- lines (a transactional outbox drained while live), giveaways.
ALTER TABLE chat_settings
    ADD COLUMN automod_caps boolean NOT NULL DEFAULT false,
    ADD COLUMN automod_repeats boolean NOT NULL DEFAULT false,
    ADD COLUMN automod_spam boolean NOT NULL DEFAULT false,
    -- Seconds per strike within an hour: 0 is a warning; the last step repeats.
    ADD COLUMN automod_ladder integer[] NOT NULL DEFAULT '{0,60,600}';
CREATE TABLE automod_strikes (
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    rule text NOT NULL,
    at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX automod_strikes_recent ON automod_strikes (channel_id, user_id, at);
CREATE TABLE bot_events (
    id bigserial PRIMARY KEY,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('follow', 'sub', 'raid')),
    name text NOT NULL,
    count integer NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE giveaways (
    id text PRIMARY KEY,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    keyword text NOT NULL CHECK (char_length(keyword) BETWEEN 1 AND 25),
    started_by text REFERENCES users(id) ON DELETE SET NULL,
    started_at timestamptz NOT NULL DEFAULT now(),
    ended_at timestamptz,
    winner_id text REFERENCES users(id) ON DELETE SET NULL,
    entries integer NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX giveaways_open ON giveaways (channel_id) WHERE ended_at IS NULL;
CREATE TABLE giveaway_entries (
    giveaway_id text NOT NULL REFERENCES giveaways(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (giveaway_id, user_id)
);
