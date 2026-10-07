-- Module 9 (docs/BEACONS.md): short vertical videos that lead viewers to creators and live streams.
CREATE TABLE beacons (
    id text PRIMARY KEY,
    owner_id text REFERENCES users(id) ON DELETE SET NULL,
    -- A viewer's approved clip becomes one of the channel's Beacons, with the clipper credited.
    clipper_id text REFERENCES users(id) ON DELETE SET NULL,
    clip_id text REFERENCES videos(id) ON DELETE SET NULL,
    broadcast_id text REFERENCES broadcasts(id) ON DELETE SET NULL,
    source text NOT NULL CHECK (source IN ('CLIP','UPLOAD')),
    status text NOT NULL CHECK (status IN ('DRAFT','PROCESSING','READY','PUBLISHED','REMOVED','FAILED','DELETING','DELETED')),
    failure text,
    publish boolean NOT NULL DEFAULT true,
    -- Counts toward the 10-per-day limit; failures and abandoned uploads give the slot back.
    quota boolean NOT NULL DEFAULT true,
    hidden boolean NOT NULL DEFAULT false,
    title text NOT NULL CHECK (char_length(title) BETWEEN 1 AND 120),
    category_id text,
    category text,
    genre text,
    mature boolean NOT NULL DEFAULT false,
    charity_name text,
    charity_url text,
    -- The 9:16 crop window over the source, as fractions of its width and height.
    crop jsonb,
    -- Per-Beacon seed for the watermark's corner order and timing.
    seed bigint NOT NULL,
    duration_ms bigint NOT NULL DEFAULT 0 CHECK (duration_ms >= 0),
    -- Source and output hashes; a legal removal adds them to the exact-copy blocklist.
    hashes text[] NOT NULL DEFAULT '{}',
    upload_key text,
    upload_until timestamptz,
    mp4_key text,
    mp4_small_key text,
    clean_key text,
    thumbnail_key text,
    revision bigint NOT NULL DEFAULT 0,
    views bigint NOT NULL DEFAULT 0,
    likes bigint NOT NULL DEFAULT 0,
    request_key text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    published_at timestamptz,
    UNIQUE(owner_id,request_key)
);
CREATE INDEX beacons_feed ON beacons(published_at DESC) WHERE status='PUBLISHED' AND NOT hidden;
CREATE INDEX beacons_owner ON beacons(owner_id,created_at DESC);
CREATE INDEX beacons_clip ON beacons(clip_id) WHERE clip_id IS NOT NULL;
-- Every stored object a Beacon owns, so deletion and Take It Down reach every copy.
CREATE TABLE beacon_objects (
    key text PRIMARY KEY,
    beacon_id text NOT NULL REFERENCES beacons(id),
    content_type text NOT NULL,
    bytes bigint NOT NULL DEFAULT 0,
    ready boolean NOT NULL DEFAULT false,
    deleted_at timestamptz
);
CREATE INDEX beacon_objects_owner ON beacon_objects(beacon_id);
-- Exactly-once processing: a lease, a token fence and retries with backoff.
CREATE TABLE beacon_jobs (
    id text PRIMARY KEY,
    beacon_id text NOT NULL REFERENCES beacons(id),
    kind text NOT NULL CHECK (kind IN ('PROCESS','DELETE')),
    input jsonb NOT NULL DEFAULT '{}',
    available_at timestamptz NOT NULL DEFAULT now(),
    lease_until timestamptz,
    lease_token text,
    attempts integer NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(beacon_id,kind)
);
CREATE INDEX beacon_jobs_due ON beacon_jobs(available_at,lease_until);
-- The crop a creator last used, remembered for their next clip Beacon.
CREATE TABLE beacon_crops (
    owner_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    crop jsonb NOT NULL
);
CREATE TABLE beacon_likes (
    beacon_id text NOT NULL REFERENCES beacons(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(beacon_id,user_id)
);
-- One playback lease per account or guest session per Beacon per day.
CREATE TABLE beacon_playback (
    beacon_id text NOT NULL REFERENCES beacons(id) ON DELETE CASCADE,
    viewer_key text NOT NULL,
    day date NOT NULL,
    network text NOT NULL,
    updated_at timestamptz NOT NULL DEFAULT now(),
    media_time double precision NOT NULL,
    watched_seconds double precision NOT NULL DEFAULT 0,
    interval_count integer NOT NULL DEFAULT 0,
    interval_mean double precision NOT NULL DEFAULT 0,
    interval_m2 double precision NOT NULL DEFAULT 0,
    turnstile_ok boolean NOT NULL DEFAULT false,
    counted boolean NOT NULL DEFAULT false,
    completed boolean NOT NULL DEFAULT false,
    PRIMARY KEY(beacon_id,viewer_key,day)
);
-- Creator-only results: completions, follows and live joins that came from a Beacon. A Live now
-- tap becomes a LIVE_JOIN once the same viewer has a counted session on the creator's stream.
CREATE TABLE beacon_events (
    beacon_id text NOT NULL REFERENCES beacons(id) ON DELETE CASCADE,
    viewer_key text NOT NULL,
    kind text NOT NULL CHECK (kind IN ('COMPLETION','FOLLOW','LIVE_TAP','LIVE_JOIN')),
    owner_id text NOT NULL,
    day date NOT NULL DEFAULT current_date,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(beacon_id,viewer_key,kind,day)
);
CREATE INDEX beacon_events_owner ON beacon_events(owner_id,kind,created_at);
CREATE TABLE beacon_mutes (
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    muted_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(user_id,muted_id),
    CHECK (user_id <> muted_id)
);
ALTER TABLE reports DROP CONSTRAINT IF EXISTS reports_target_type_check;
ALTER TABLE reports ADD CONSTRAINT reports_target_type_check CHECK (target_type IN ('profile','wall_post','wall_reply','fan_art','setup_photo','chat_message','live_stream','emote','faction_post','guild','guild_emblem','vod','highlight','clip','beacon'));
