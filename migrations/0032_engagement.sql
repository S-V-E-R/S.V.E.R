-- Module 6 Support, part 3 (docs/SUPPORT.md "Engagement Valor"): per-channel loyalty points and
-- channel rewards. Engagement Valor has no cash value, so it lives here rather than in the ledger.

-- One balance per viewer per channel, earned by verified viewers by watching, chatting and a
-- one-time follow bonus (follow_bonus stops refollow farming).
CREATE TABLE engagement (
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    balance bigint NOT NULL DEFAULT 0 CHECK (balance >= 0),
    earned bigint NOT NULL DEFAULT 0 CHECK (earned >= 0),
    last_watch_at timestamptz,
    last_chat_at timestamptz,
    follow_bonus boolean NOT NULL DEFAULT false,
    PRIMARY KEY (channel_id, user_id),
    CHECK (channel_id <> user_id)
);
CREATE INDEX engagement_user ON engagement (user_id);

-- Rewards an owner offers. 'highlight' is the built-in "highlight my message" reward (one per
-- channel); a prompt means the viewer must enter text.
CREATE TABLE channel_rewards (
    id text PRIMARY KEY,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind text NOT NULL DEFAULT 'custom' CHECK (kind IN ('custom', 'highlight')),
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 45),
    cost integer NOT NULL CHECK (cost BETWEEN 1 AND 1000000),
    cooldown_seconds integer NOT NULL DEFAULT 0 CHECK (cooldown_seconds BETWEEN 0 AND 604800),
    per_stream_limit integer CHECK (per_stream_limit BETWEEN 1 AND 1000),
    prompt text CHECK (prompt IS NULL OR char_length(prompt) BETWEEN 1 AND 100),
    enabled boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX channel_rewards_channel ON channel_rewards (channel_id, created_at);
CREATE UNIQUE INDEX channel_rewards_highlight ON channel_rewards (channel_id) WHERE kind = 'highlight';

-- Each redemption, with the reward's name and cost at the time. Owners and moderators mark them
-- done or refund them (refunds return the points). Highlights are done when the message posts.
CREATE TABLE reward_redemptions (
    id text PRIMARY KEY,
    reward_id text REFERENCES channel_rewards(id) ON DELETE SET NULL,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name text NOT NULL,
    cost integer NOT NULL,
    input text CHECK (input IS NULL OR char_length(input) BETWEEN 1 AND 200),
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'done', 'refunded')),
    created_at timestamptz NOT NULL DEFAULT now(),
    resolved_at timestamptz,
    resolved_by text REFERENCES users(id) ON DELETE SET NULL
);
CREATE INDEX reward_redemptions_queue ON reward_redemptions (channel_id, status, created_at);
CREATE INDEX reward_redemptions_reward ON reward_redemptions (reward_id, created_at);

ALTER TABLE chat_messages ADD COLUMN highlighted boolean NOT NULL DEFAULT false;
