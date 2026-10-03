-- Module 3 chat. One row per message; `seq` orders joins against live events, the
-- client-generated `id` makes a retried send idempotent. Bodies expire after seven days;
-- report snapshots keep their own copy under the Module 2 retention rule.
CREATE TABLE chat_messages (
    id text PRIMARY KEY,
    seq bigint GENERATED ALWAYS AS IDENTITY UNIQUE,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    author_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body text NOT NULL CHECK (char_length(body) BETWEEN 1 AND 500),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL DEFAULT now() + interval '7 days',
    deleted_at timestamptz
);
CREATE INDEX chat_messages_history ON chat_messages(channel_id, seq DESC) WHERE deleted_at IS NULL;
CREATE INDEX chat_messages_expiry ON chat_messages(expires_at);
