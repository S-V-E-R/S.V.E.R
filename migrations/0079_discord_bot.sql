-- The S.V.E.R Discord bot (docs/COMMUNITY.md "Discord bot"): one Discord server per channel,
-- go-live posts, and the roles the bot keeps in sync.
CREATE TABLE discord_servers (
    channel_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    guild_id text NOT NULL,
    guild_name text NOT NULL DEFAULT '',
    post_channel text,
    sub_role text,
    guild_role text,
    faction_roles jsonb NOT NULL DEFAULT '{}',
    problem text,
    synced_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
-- One go-live post per broadcast, retried like webhooks.
CREATE TABLE discord_posts (
    id bigserial PRIMARY KEY,
    broadcast_id text NOT NULL UNIQUE REFERENCES broadcasts(id) ON DELETE CASCADE,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    attempts integer NOT NULL DEFAULT 0,
    available_at timestamptz NOT NULL DEFAULT now(),
    delivered_at timestamptz,
    error text,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX discord_posts_due ON discord_posts(available_at) WHERE delivered_at IS NULL;
-- Roles the bot gave, so it only ever takes back its own.
CREATE TABLE discord_grants (
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    discord_user text NOT NULL,
    role_id text NOT NULL,
    guild_id text NOT NULL,
    PRIMARY KEY (channel_id, discord_user, role_id)
);
