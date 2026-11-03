-- Viewer analytics (docs/ANALYTICS.md). Shown to the streamer as totals only, never per viewer,
-- and never used for discovery, MAGNet, factions or money. Both tables are kept 90 days.

-- One row per visit: a run of playback heartbeats on one broadcast by one viewer, ending when
-- the beats stop for 30 seconds (a viewer who leaves and comes back starts a new visit).
CREATE TABLE watch_visits (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    broadcast_id text NOT NULL REFERENCES broadcasts(id) ON DELETE CASCADE,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- The playback lease's key: 'u:<user id>' or 'b:<digest of a random browser ID>'.
    viewer_key text NOT NULL,
    -- How the viewer reached the player (docs/ANALYTICS.md "Sources").
    source text NOT NULL CHECK (source IN ('home','magnet_channel','magnet','search','browse',
        'following','alert','suggestion','raid','host','beacon','clip','video','faction','channel',
        'external','direct','other')),
    -- For 'external': the referring site's host name only, never a path.
    referrer_host text CHECK (length(referrer_host) <= 253),
    -- Whether the viewer followed the channel when the visit started.
    follower boolean NOT NULL DEFAULT false,
    started_at timestamptz NOT NULL DEFAULT now(),
    last_at timestamptz NOT NULL DEFAULT now(),
    -- Seconds of playback (heartbeats only arrive while media advances, each worth at most 15 s).
    seconds double precision NOT NULL DEFAULT 0,
    -- Whether viewer integrity counted the session (Counted or Trusted); only these are shown.
    counted boolean NOT NULL DEFAULT false
);
CREATE INDEX watch_visits_open ON watch_visits(broadcast_id, viewer_key, last_at);
CREATE INDEX watch_visits_channel ON watch_visits(channel_id, started_at);
CREATE INDEX watch_visits_started ON watch_visits(started_at);

-- Live cards a viewer actually saw (on screen for a second), per broadcast, surface and hour.
CREATE TABLE discovery_impressions (
    broadcast_id text NOT NULL REFERENCES broadcasts(id) ON DELETE CASCADE,
    surface text NOT NULL CHECK (surface IN ('home','browse','search','following','faction','magnet','suggestion')),
    hour timestamptz NOT NULL,
    shown integer NOT NULL DEFAULT 0,
    PRIMARY KEY (broadcast_id, surface, hour)
);
CREATE INDEX discovery_impressions_hour ON discovery_impressions(hour);
