-- Module 7 CrowdSync, phase 2 (docs/CROWDSYNC.md "Polls and predictions", "Counter widgets"):
-- one poll system (polls and Engagement Valor predictions) and the streamer's counters.

-- A poll ends, and a prediction locks, at `ends_at`; votes are accepted a few seconds after it so
-- viewers whose video runs behind get the same window (the API adds the grace).
CREATE TABLE polls (
    id text PRIMARY KEY,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('poll', 'prediction')),
    question text NOT NULL CHECK (char_length(question) BETWEEN 1 AND 120),
    options jsonb NOT NULL,
    created_by text REFERENCES users(id) ON DELETE SET NULL,
    broadcast_id text NOT NULL,
    ends_at timestamptz NOT NULL,
    -- open -> ended (polls) | open -> locked -> resolved | cancelled (predictions)
    status text NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'ended', 'locked', 'resolved', 'cancelled')),
    winner smallint,
    created_at timestamptz NOT NULL DEFAULT now(),
    closed_at timestamptz
);
-- At most one running poll and one running prediction per channel.
CREATE UNIQUE INDEX polls_running ON polls (channel_id, kind) WHERE status IN ('open', 'locked');
CREATE INDEX polls_recent ON polls (channel_id, created_at DESC);

-- One vote per viewer; a prediction's stake is Engagement Valor, paid back as `payout` on a win
-- (or refunded when cancelled).
CREATE TABLE poll_votes (
    poll_id text NOT NULL REFERENCES polls(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    option smallint NOT NULL,
    stake integer NOT NULL DEFAULT 0 CHECK (stake >= 0),
    payout integer,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (poll_id, user_id)
);

-- Counter widgets: shiny (value = encounters, extra = phase, odds = 1 in N), deaths, a win/loss
-- tally (value = wins, extra = losses) and custom counters.
CREATE TABLE counters (
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    id text NOT NULL CHECK (id ~ '^[a-z0-9]{1,16}$'),
    kind text NOT NULL CHECK (kind IN ('shiny', 'deaths', 'tally', 'custom')),
    label text NOT NULL CHECK (char_length(label) BETWEEN 1 AND 40),
    value integer NOT NULL DEFAULT 0 CHECK (value BETWEEN 0 AND 1000000000),
    extra integer NOT NULL DEFAULT 0 CHECK (extra BETWEEN 0 AND 1000000000),
    odds integer CHECK (odds BETWEEN 2 AND 1000000),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (channel_id, id)
);
