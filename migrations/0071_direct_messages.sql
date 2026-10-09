-- Direct messages (docs/COMMUNITY.md "Direct messages"). Bodies are sealed by the API with a
-- DM-only purpose binding, so a database copy alone reveals nothing. Messages expire after 12
-- months unless an open report holds them; a conversation goes with either person's account.
ALTER TABLE users ADD COLUMN dm_policy text CHECK (dm_policy IN ('mutuals', 'following', 'nobody'));
CREATE TABLE dm_conversations (
    id text PRIMARY KEY,
    user_a text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_b text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    last_message_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (user_a, user_b),
    CHECK (user_a < user_b)
);
CREATE INDEX dm_conversations_b ON dm_conversations (user_b);
-- Per person: what they've read, what they cleared from their own view, and notification mute.
CREATE TABLE dm_state (
    conversation_id text NOT NULL REFERENCES dm_conversations(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    read_seq bigint NOT NULL DEFAULT 0,
    cleared_seq bigint NOT NULL DEFAULT 0,
    muted boolean NOT NULL DEFAULT false,
    PRIMARY KEY (conversation_id, user_id)
);
CREATE TABLE dm_messages (
    id text PRIMARY KEY,
    seq bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
    conversation_id text NOT NULL REFERENCES dm_conversations(id) ON DELETE CASCADE,
    sender_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body_sealed text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL DEFAULT now() + interval '12 months',
    deleted_at timestamptz
);
CREATE INDEX dm_messages_conversation ON dm_messages (conversation_id, seq DESC);
CREATE INDEX dm_messages_expiry ON dm_messages (expires_at);
ALTER TABLE reports DROP CONSTRAINT reports_target_type_check;
ALTER TABLE reports ADD CONSTRAINT reports_target_type_check CHECK (target_type IN ('profile','wall_post','wall_reply','fan_art','setup_photo','chat_message','live_stream','emote','faction_post','guild','guild_emblem','vod','highlight','clip','beacon','dm_message'));
