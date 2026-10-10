-- Chat commands and the channel bot, part 1 (docs/COMMUNITY.md): custom !commands, timed messages
-- and the channel's bot persona (NULL is the owner's faction's bot, else VOLK).
CREATE TABLE chat_commands (
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (name ~ '^[a-z0-9_]{1,25}$'),
    reply text NOT NULL CHECK (char_length(reply) BETWEEN 1 AND 300),
    access text NOT NULL DEFAULT 'everyone' CHECK (access IN ('everyone', 'followers', 'subscribers', 'moderators')),
    cooldown_seconds integer NOT NULL DEFAULT 10 CHECK (cooldown_seconds BETWEEN 0 AND 3600),
    uses bigint NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (channel_id, name)
);
CREATE TABLE chat_timers (
    id text PRIMARY KEY,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body text NOT NULL CHECK (char_length(body) BETWEEN 1 AND 300),
    every_minutes integer NOT NULL CHECK (every_minutes BETWEEN 10 AND 1440),
    enabled boolean NOT NULL DEFAULT true,
    last_sent_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX chat_timers_channel ON chat_timers (channel_id);
ALTER TABLE chat_settings
    ADD COLUMN bot text CHECK (bot IN ('pyre', 'echo', 'favor', 'volk')),
    ADD COLUMN bot_personality text NOT NULL DEFAULT 'chill' CHECK (bot_personality IN ('chill', 'battle', 'event'));
