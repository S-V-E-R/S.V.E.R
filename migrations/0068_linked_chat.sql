-- Linked chat (docs/LINKED_CHAT.md): a streamer's accounts on other platforms, and the chat that
-- arrives from them. Outside messages live in their own table so nothing that counts S.V.E.R chat
-- (viewers, Valor, influence, MAGNet, integrity, orders) can ever see them; they share the chat
-- sequence so they interleave with S.V.E.R messages in order.
CREATE TABLE linked_chat_accounts (
    owner_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    platform text NOT NULL CHECK (platform IN ('twitch', 'youtube', 'kick')),
    subject text NOT NULL,
    handle text NOT NULL,
    access_sealed text NOT NULL,
    refresh_sealed text,
    expires_at timestamptz,
    enabled boolean NOT NULL DEFAULT true,
    status text NOT NULL DEFAULT 'idle' CHECK (status IN ('idle', 'connecting', 'connected', 'reconnecting', 'revoked')),
    detail text,
    status_at timestamptz,
    subscriptions text[] NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (owner_id, platform)
);
CREATE UNIQUE INDEX linked_chat_accounts_subject ON linked_chat_accounts (platform, subject);
CREATE TABLE outside_chat_messages (
    id text PRIMARY KEY,
    seq bigint NOT NULL UNIQUE,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    platform text NOT NULL,
    sender_id text NOT NULL,
    sender_name text NOT NULL,
    sender_login text NOT NULL,
    sender_role text CHECK (sender_role IN ('broadcaster', 'moderator', 'subscriber')),
    body text NOT NULL CHECK (char_length(body) BETWEEN 1 AND 500),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL DEFAULT now() + interval '7 days',
    hidden_at timestamptz
);
CREATE INDEX outside_chat_recent ON outside_chat_messages (channel_id, seq DESC) WHERE hidden_at IS NULL;
DO $$ BEGIN
    EXECUTE format('ALTER TABLE outside_chat_messages ALTER seq SET DEFAULT nextval(%L)', pg_get_serial_sequence('chat_messages', 'seq'));
END $$;
-- S.V.E.R-only mutes of an outside sender: for one broadcast, or for good (broadcast_id NULL).
CREATE TABLE outside_chat_mutes (
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    platform text NOT NULL,
    sender_id text NOT NULL,
    broadcast_id text,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (channel_id, platform, sender_id)
);
-- Linking an account for chat is its own OAuth intent (chat scopes, back to Creator Studio).
ALTER TABLE oauth_states DROP CONSTRAINT oauth_states_intent_check;
ALTER TABLE oauth_states ADD CONSTRAINT oauth_states_intent_check CHECK (intent IN ('login', 'signup', 'link', 'reauth', 'chat'));
