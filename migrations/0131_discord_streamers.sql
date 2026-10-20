-- Streamers on the home server (docs/DISCORD_BOT.md "Go-live alerts and streamers"): go-live alerts
-- for S.V.E.R, Twitch, Kick and YouTube, stream sessions, streamer profiles, schedules, follower
-- milestones, spotlights, collab requests. Discord users are keyed by their Discord id.

-- One streaming account per member and platform, with its live state and the go-live post.
CREATE TABLE discord_stream_accounts (
    discord_user text NOT NULL,
    platform text NOT NULL CHECK (platform IN ('sver', 'twitch', 'youtube', 'kick')),
    handle text NOT NULL,
    -- The platform's own id (S.V.E.R user id, Twitch user id, YouTube channel id, Kick slug).
    platform_id text,
    alerts boolean NOT NULL DEFAULT true,
    -- Set while live: when it started, and which stream (broadcast or video id).
    live_since timestamptz,
    live_key text,
    title text,
    game text,
    -- The go-live post, edited to "was live" at the end and deleted 4 hours later.
    post_channel text,
    message_id text,
    posted_at timestamptz,
    offline_at timestamptz,
    -- Twitch only: our EventSub subscriptions for stream.online and stream.offline.
    eventsub text[] NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (discord_user, platform)
);
CREATE INDEX discord_stream_accounts_platform ON discord_stream_accounts(platform, platform_id);
CREATE INDEX discord_stream_accounts_posts ON discord_stream_accounts(offline_at) WHERE message_id IS NOT NULL;

CREATE TABLE discord_stream_sessions (
    id bigserial PRIMARY KEY,
    discord_user text NOT NULL,
    platform text NOT NULL,
    handle text NOT NULL,
    started_at timestamptz NOT NULL,
    ended_at timestamptz NOT NULL,
    minutes integer NOT NULL CHECK (minutes >= 0)
);
CREATE INDEX discord_stream_sessions_ended ON discord_stream_sessions(ended_at);

-- Members who unlinked an auto-detected account are never auto-linked again.
CREATE TABLE discord_autolink_optouts (
    discord_user text NOT NULL,
    platform text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (discord_user, platform)
);

CREATE TABLE discord_streamer_profiles (
    discord_user text PRIMARY KEY,
    bio text CHECK (length(bio) <= 500),
    tags text[] NOT NULL DEFAULT '{}',
    games text[] NOT NULL DEFAULT '{}',
    collab boolean NOT NULL DEFAULT false,
    collab_platforms text[] NOT NULL DEFAULT '{}',
    collab_content text[] NOT NULL DEFAULT '{}',
    collab_size smallint NOT NULL DEFAULT 4,
    collab_notes text NOT NULL DEFAULT '',
    featured boolean NOT NULL DEFAULT false,
    last_live_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- One slot per member and weekday (0 Sunday … 6 Saturday), in the member's own time zone.
CREATE TABLE discord_schedules (
    discord_user text NOT NULL,
    day smallint NOT NULL CHECK (day BETWEEN 0 AND 6),
    at time NOT NULL,
    zone text NOT NULL,
    game text,
    description text,
    platform text,
    PRIMARY KEY (discord_user, day)
);

CREATE TABLE discord_milestones (
    discord_user text NOT NULL,
    kind text NOT NULL,
    value integer NOT NULL,
    at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (discord_user, kind, value)
);

CREATE TABLE discord_spotlights (
    id bigserial PRIMARY KEY,
    kind text NOT NULL CHECK (kind IN ('streamer', 'hero', 'custom')),
    discord_user text NOT NULL,
    title text,
    reason text,
    nominated_by text,
    faction text,
    channel_id text,
    message_id text,
    featured boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX discord_spotlights_kind ON discord_spotlights(kind, created_at DESC);

CREATE TABLE discord_collabs (
    id bigserial PRIMARY KEY,
    discord_user text NOT NULL,
    platform text NOT NULL CHECK (platform IN ('sver', 'twitch', 'youtube', 'kick', 'any')),
    game text,
    schedule text,
    description text NOT NULL CHECK (length(description) BETWEEN 1 AND 500),
    max_responses smallint NOT NULL DEFAULT 5,
    status text NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'filled', 'expired', 'cancelled')),
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL DEFAULT now() + interval '7 days',
    filled_at timestamptz
);
-- One open request per member.
CREATE UNIQUE INDEX discord_collabs_one_open ON discord_collabs(discord_user) WHERE status = 'open';
CREATE TABLE discord_collab_responses (
    collab_id bigint NOT NULL REFERENCES discord_collabs(id) ON DELETE CASCADE,
    discord_user text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (collab_id, discord_user)
);

-- Weekly posts that run once per period (schedule, clip of the week, spotlight).
CREATE TABLE discord_weekly_runs (
    job text NOT NULL,
    period date NOT NULL,
    PRIMARY KEY (job, period)
);
