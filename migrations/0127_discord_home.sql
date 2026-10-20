-- The community bot on the S.V.E.R home server (docs/DISCORD_BOT.md): its Gateway connection, the
-- servers it's in, and the home server's settings.
CREATE TABLE discord_gateway (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    connected boolean NOT NULL DEFAULT false,
    connected_at timestamptz,
    last_event_at timestamptz,
    problem text,
    application_id text,
    bot_id text,
    bot_name text,
    guild_ids text[] NOT NULL DEFAULT '{}'
);
CREATE TABLE discord_guilds (
    guild_id text PRIMARY KEY,
    name text NOT NULL,
    seen_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE discord_home (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    guild_id text NOT NULL,
    settings jsonb NOT NULL DEFAULT '{}',
    updated_by text REFERENCES users(id) ON DELETE SET NULL,
    updated_at timestamptz NOT NULL DEFAULT now()
);
