-- Module 3 channel moderation: appointed moderators, chat timeouts/bans, chat rules and
-- an audit log. Channel roles never grant /admin access.
CREATE TABLE channel_moderators (
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    appointed_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (channel_id, user_id),
    CHECK (channel_id <> user_id)
);
-- A timeout has an end; a ban lasts until lifted. A new timeout replaces the old end time
-- and never clears a ban. A ban also refuses signed-in playback (Joe, October 3, 2026).
CREATE TABLE channel_restrictions (
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('timeout', 'ban')),
    until timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (channel_id, user_id, kind),
    CHECK ((kind = 'ban') = (until IS NULL))
);
CREATE TABLE chat_settings (
    channel_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    slow_mode_seconds integer NOT NULL DEFAULT 0 CHECK (slow_mode_seconds BETWEEN 0 AND 3600),
    block_links boolean NOT NULL DEFAULT false,
    banned_words text[] NOT NULL DEFAULT '{}' CHECK (cardinality(banned_words) <= 200)
);
CREATE TABLE channel_moderation_log (
    id text PRIMARY KEY,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    actor_id text REFERENCES users(id) ON DELETE SET NULL,
    actor_role text NOT NULL CHECK (actor_role IN ('owner', 'moderator', 'staff')),
    action text NOT NULL,
    target_id text REFERENCES users(id) ON DELETE SET NULL,
    message_id text,
    detail jsonb NOT NULL DEFAULT '{}',
    reason text NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 500),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX channel_moderation_log_recent ON channel_moderation_log(channel_id, created_at DESC);
