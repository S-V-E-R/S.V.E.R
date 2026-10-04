-- Module 3 raids and hosting (docs/LIVE_STREAMS.md "Raids" and "Hosting").
-- No row means the defaults: raids and hosts accepted, auto-host off.
CREATE TABLE channel_host_settings (
    user_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    accept_raids boolean NOT NULL DEFAULT true,
    accept_hosts boolean NOT NULL DEFAULT true,
    auto_host boolean NOT NULL DEFAULT false,
    -- Priority order of user IDs for auto-host.
    auto_list text[] NOT NULL DEFAULT '{}' CHECK (cardinality(auto_list) <= 10)
);
-- Channels a channel won't accept raids or hosts from.
CREATE TABLE raid_blocks (
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    blocked_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (channel_id, blocked_id),
    CHECK (channel_id <> blocked_id)
);
-- Also the raid audit: raider, target, time and arrivals.
CREATE TABLE raids (
    id text PRIMARY KEY,
    raider_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    broadcast_id text NOT NULL REFERENCES broadcasts(id) ON DELETE CASCADE,
    target_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    target_broadcast_id text NOT NULL REFERENCES broadcasts(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    execute_at timestamptz NOT NULL,
    status text NOT NULL DEFAULT 'countdown' CHECK (status IN ('countdown', 'cancelled', 'moved', 'failed')),
    arrivals integer,
    counted_at timestamptz,
    -- The raider's channel hosts the target after their broadcast ends (once).
    hosted boolean NOT NULL DEFAULT false,
    CHECK (raider_id <> target_id)
);
CREATE UNIQUE INDEX one_raid_countdown ON raids (broadcast_id) WHERE status = 'countdown';
CREATE INDEX raids_recent ON raids (broadcast_id, created_at DESC);
CREATE INDEX raids_target ON raids (target_broadcast_id, execute_at DESC);
CREATE INDEX raids_open ON raids (status, execute_at) WHERE status IN ('countdown', 'moved');
ALTER TABLE playback_leases ADD COLUMN raid_id text REFERENCES raids(id) ON DELETE SET NULL;
-- An offline channel hosting one live channel.
CREATE TABLE host_state (
    host_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    target_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    source text NOT NULL CHECK (source IN ('manual', 'auto', 'raid')),
    started_at timestamptz NOT NULL DEFAULT now(),
    CHECK (host_id <> target_id)
);
CREATE INDEX host_state_target ON host_state (target_id);

-- The one definition of who may send viewers (a raid or a host) to whom; NULL means allowed.
-- Liveness is checked by the caller. Blocks in either direction, a raid block or a channel ban
-- all read 'blocked', so the sender can't tell which applies.
CREATE FUNCTION viewer_send_refusal(from_id text, to_id text, hosting boolean) RETURNS text
LANGUAGE sql STABLE AS $$
    SELECT CASE
        WHEN from_id = to_id THEN 'self'
        WHEN NOT coalesce((SELECT eligible FROM channel_users WHERE id = to_id), false) THEN 'unavailable'
        WHEN NOT coalesce((SELECT eligible FROM channel_users WHERE id = from_id), false) THEN 'unavailable'
        WHEN coalesce((SELECT CASE WHEN hosting THEN NOT accept_hosts ELSE NOT accept_raids END
                       FROM channel_host_settings WHERE user_id = to_id), false) THEN 'closed'
        WHEN EXISTS (SELECT 1 FROM raid_blocks WHERE channel_id = to_id AND blocked_id = from_id)
          OR EXISTS (SELECT 1 FROM user_blocks WHERE (blocker_id = to_id AND blocked_id = from_id)
                                                OR (blocker_id = from_id AND blocked_id = to_id))
          OR EXISTS (SELECT 1 FROM channel_restrictions WHERE channel_id = to_id AND user_id = from_id AND kind = 'ban')
            THEN 'blocked'
    END
$$;
