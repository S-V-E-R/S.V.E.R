-- Module 9: Beacons (docs/BEACONS.md). A Beacon is a video row (kind BEACON, parent = its source
-- clip), so storage, leased jobs, holds, Take It Down and deletion work as for every other video.
ALTER TABLE videos DROP CONSTRAINT videos_kind_check;
ALTER TABLE videos ADD CONSTRAINT videos_kind_check CHECK (kind IN ('VOD','HIGHLIGHT','CLIP','BEACON'));
ALTER TABLE video_jobs DROP CONSTRAINT video_jobs_kind_check;
ALTER TABLE video_jobs ADD CONSTRAINT video_jobs_kind_check CHECK (kind IN ('SEGMENT','ASSEMBLE','THUMBNAIL','DOWNLOAD','DELETE','BEACON'));
CREATE TABLE beacons (
    video_id text PRIMARY KEY REFERENCES videos(id),
    source_id text NOT NULL REFERENCES videos(id),
    -- Position of the 9:16 window along the source's free axis (0 = left/top, 1 = right/bottom).
    crop real NOT NULL CHECK (crop BETWEEN 0 AND 1),
    hd_key text,
    sd_key text,
    clean_key text,
    failure text,
    published_at timestamptz,
    likes bigint NOT NULL DEFAULT 0 CHECK (likes >= 0),
    -- For the creator only; never read by feed ordering.
    completions bigint NOT NULL DEFAULT 0,
    follows bigint NOT NULL DEFAULT 0,
    live_joins bigint NOT NULL DEFAULT 0
);
CREATE INDEX beacon_feed ON beacons(published_at DESC) WHERE published_at IS NOT NULL;
CREATE INDEX beacon_source ON beacons(source_id);
CREATE TABLE beacon_likes (
    beacon_id text NOT NULL REFERENCES beacons(video_id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(beacon_id,user_id)
);
CREATE TABLE beacon_mutes (
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    creator_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY(user_id,creator_id)
);
-- Each viewer counts once per Beacon and kind.
CREATE TABLE beacon_events (
    beacon_id text NOT NULL REFERENCES beacons(video_id) ON DELETE CASCADE,
    viewer_key text NOT NULL,
    kind text NOT NULL CHECK (kind IN ('COMPLETE','FOLLOW','LIVE')),
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(beacon_id,viewer_key,kind)
);
ALTER TABLE reports DROP CONSTRAINT IF EXISTS reports_target_type_check;
ALTER TABLE reports ADD CONSTRAINT reports_target_type_check CHECK (target_type IN ('profile','wall_post','wall_reply','fan_art','setup_photo','chat_message','live_stream','emote','faction_post','guild','guild_emblem','vod','highlight','clip','beacon'));
