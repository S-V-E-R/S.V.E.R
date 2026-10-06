-- Module 7 CrowdSync, phase 3 (docs/CROWDSYNC.md "Skills", "Faction Rally and emote combos",
-- "Surge"). Emote combos are counted in memory and need no table.

-- A Skill is a chat message paid in Purchased Valor like a tribute; this names the Skill played.
ALTER TABLE chat_messages ADD COLUMN skill text;
-- Skill categories a streamer switched off for their channel ('sticker', 'fullscreen', 'sound').
CREATE TABLE skill_settings (
    channel_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    disabled text[] NOT NULL DEFAULT '{}'
);

-- Faction Rally: at most one rally per person per minute on a broadcast; the meter is each
-- faction's share for that broadcast.
CREATE TABLE rallies (
    broadcast_id text NOT NULL,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    faction text NOT NULL CHECK (faction IN ('myria', 'aetheron', 'glint')),
    minute bigint NOT NULL,
    PRIMARY KEY (broadcast_id, user_id, minute)
);

-- Surge: one participation per real viewer per minute (chatting, rallying, pressing, tributes,
-- Skills, subscribing), never weighted by amount.
CREATE TABLE surge_participation (
    broadcast_id text NOT NULL,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    minute bigint NOT NULL,
    PRIMARY KEY (broadcast_id, user_id, minute)
);
CREATE INDEX surge_participation_recent ON surge_participation (broadcast_id, minute);
CREATE TABLE surges (
    id text PRIMARY KEY,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    broadcast_id text NOT NULL,
    level smallint NOT NULL DEFAULT 1 CHECK (level BETWEEN 1 AND 5),
    threshold integer NOT NULL,
    started_at timestamptz NOT NULL DEFAULT now(),
    ends_at timestamptz NOT NULL,
    ended_at timestamptz
);
CREATE UNIQUE INDEX surges_running ON surges (channel_id) WHERE ended_at IS NULL;
CREATE INDEX surges_recent ON surges (channel_id, started_at DESC);
-- Engagement Valor awarded when a Surge ends, capped per viewer per channel per day.
CREATE TABLE surge_awards (
    surge_id text NOT NULL REFERENCES surges(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    channel_id text NOT NULL,
    day date NOT NULL,
    amount integer NOT NULL CHECK (amount > 0),
    PRIMARY KEY (surge_id, user_id)
);
CREATE INDEX surge_awards_daily ON surge_awards (channel_id, user_id, day);
