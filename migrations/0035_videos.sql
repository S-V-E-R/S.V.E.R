-- Module 8 owns recording metadata, durable media work and private object ownership.
CREATE TABLE video_settings (
    owner_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    recording boolean NOT NULL DEFAULT true,
    visibility text NOT NULL DEFAULT 'PUBLIC' CHECK (visibility IN ('PUBLIC','SUBSCRIBERS','PRIVATE')),
    clip_permission text NOT NULL DEFAULT 'SIGNED_IN' CHECK (clip_permission IN ('SIGNED_IN','FOLLOWERS','SUBSCRIBERS','MODS','OFF')),
    clip_approval boolean NOT NULL DEFAULT false,
    chat_replay boolean NOT NULL DEFAULT true,
    mature boolean NOT NULL DEFAULT false,
    copyright_restricted boolean NOT NULL DEFAULT false
);
CREATE TABLE video_editors (
    owner_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    editor_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY(owner_id,editor_id)
);
CREATE TABLE videos (
    id text PRIMARY KEY,
    owner_id text REFERENCES users(id) ON DELETE SET NULL,
    clipper_id text REFERENCES users(id) ON DELETE SET NULL,
    broadcast_id text REFERENCES broadcasts(id) ON DELETE SET NULL,
    parent_id text REFERENCES videos(id),
    kind text NOT NULL CHECK (kind IN ('VOD','HIGHLIGHT','CLIP')),
    status text NOT NULL CHECK (status IN ('RECORDING','PROCESSING','READY','DELETING','EXPIRED','DELETED','FAILED')),
    approval text NOT NULL DEFAULT 'APPROVED' CHECK (approval IN ('PENDING','APPROVED','REJECTED')),
    visibility text NOT NULL CHECK (visibility IN ('PUBLIC','SUBSCRIBERS','PRIVATE')),
    recording boolean NOT NULL DEFAULT true,
    mature boolean NOT NULL DEFAULT false,
    title text NOT NULL CHECK (length(title) BETWEEN 1 AND 140),
    category_id text,
    category text,
    genre text,
    faction text,
    started_at timestamptz NOT NULL,
    ended_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz,
    retention_hours integer NOT NULL DEFAULT 24,
    duration_ms bigint NOT NULL DEFAULT 0 CHECK (duration_ms >= 0),
    source_start_ms bigint NOT NULL DEFAULT 0,
    source_end_ms bigint,
    revision bigint NOT NULL DEFAULT 0,
    views bigint NOT NULL DEFAULT 0,
    thumbnail_key text,
    mp4_key text,
    download_key text,
    download_until timestamptz,
    beacon_approved boolean NOT NULL DEFAULT false,
    request_key text,
    UNIQUE(clipper_id,request_key),
    UNIQUE(broadcast_id,kind,id)
);
CREATE UNIQUE INDEX video_one_recording ON videos(broadcast_id) WHERE kind='VOD';
CREATE INDEX video_channel ON videos(owner_id,created_at DESC);
CREATE INDEX video_retention ON videos(expires_at) WHERE status IN ('READY','RECORDING');
CREATE TABLE video_objects (
    key text PRIMARY KEY,
    video_id text NOT NULL REFERENCES videos(id),
    content_type text NOT NULL,
    bytes bigint NOT NULL DEFAULT 0,
    ready boolean NOT NULL DEFAULT false,
    delete_after timestamptz,
    deleted_at timestamptz
);
CREATE INDEX video_object_owner ON video_objects(video_id);
CREATE INDEX video_object_cleanup ON video_objects(delete_after) WHERE delete_after IS NOT NULL;
CREATE TABLE video_segments (
    id text PRIMARY KEY,
    video_id text NOT NULL REFERENCES videos(id),
    object_key text NOT NULL UNIQUE REFERENCES video_objects(key),
    source_identity text NOT NULL,
    start_ms bigint NOT NULL CHECK (start_ms >= 0),
    duration_ms bigint NOT NULL CHECK (duration_ms BETWEEN 1 AND 30000),
    wall_start timestamptz NOT NULL,
    discontinuity boolean NOT NULL DEFAULT false,
    UNIQUE(video_id,source_identity),
    UNIQUE(video_id,start_ms)
);
CREATE TABLE video_jobs (
    id text PRIMARY KEY,
    video_id text NOT NULL REFERENCES videos(id),
    kind text NOT NULL CHECK (kind IN ('SEGMENT','ASSEMBLE','THUMBNAIL','DOWNLOAD','DELETE')),
    object_key text,
    payload bytea,
    input jsonb NOT NULL DEFAULT '{}',
    available_at timestamptz NOT NULL DEFAULT now(),
    lease_until timestamptz,
    lease_token text,
    attempts integer NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(kind,object_key)
);
CREATE INDEX video_job_due ON video_jobs(available_at,lease_until);
CREATE INDEX video_job_video ON video_jobs(video_id);
-- Pin sources at creation so expiry cannot race a queued clip or Highlight.
CREATE TABLE video_copy_sources (
    video_id text NOT NULL REFERENCES videos(id),
    position integer NOT NULL,
    segment_id text NOT NULL REFERENCES video_segments(id),
    PRIMARY KEY(video_id,position)
);
CREATE TABLE video_chapters (
    id text PRIMARY KEY,
    video_id text NOT NULL REFERENCES videos(id),
    offset_ms bigint NOT NULL CHECK (offset_ms >= 0),
    label text NOT NULL CHECK (length(label) BETWEEN 1 AND 100),
    source text NOT NULL CHECK (source IN ('CATEGORY','MAGNET','MARKER')),
    source_id text,
    UNIQUE(video_id,source,source_id)
);
CREATE TABLE video_chat (
    video_id text NOT NULL REFERENCES videos(id),
    message_id text NOT NULL,
    author_id text REFERENCES users(id) ON DELETE SET NULL,
    offset_ms bigint NOT NULL,
    message jsonb NOT NULL,
    PRIMARY KEY(video_id,message_id)
);
CREATE INDEX video_chat_message ON video_chat(message_id);
-- Holds revoke delivery immediately and keep private evidence past normal expiry.
CREATE TABLE video_holds (
    video_id text NOT NULL REFERENCES videos(id),
    kind text NOT NULL CHECK (kind IN ('REPORT','TAKE_DOWN','COPYRIGHT')),
    reference text NOT NULL,
    permanent boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(video_id,kind,reference)
);
CREATE TABLE video_playback (
    video_id text NOT NULL REFERENCES videos(id),
    viewer_key text NOT NULL,
    network text NOT NULL,
    started_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    media_time double precision NOT NULL,
    watched_seconds double precision NOT NULL DEFAULT 0,
    interval_count integer NOT NULL DEFAULT 0,
    interval_mean double precision NOT NULL DEFAULT 0,
    interval_m2 double precision NOT NULL DEFAULT 0,
    turnstile_ok boolean NOT NULL DEFAULT false,
    hard boolean NOT NULL DEFAULT false,
    counted boolean NOT NULL DEFAULT false,
    PRIMARY KEY(video_id,viewer_key)
);
CREATE TABLE copyright_cases (
    id text PRIMARY KEY,
    video_id text NOT NULL REFERENCES videos(id),
    owner_id text REFERENCES users(id) ON DELETE SET NULL,
    status text NOT NULL DEFAULT 'OPEN' CHECK (status IN ('OPEN','REMOVED','REJECTED','COUNTER_PENDING','COUNTER','LITIGATION','RESTORED')),
    notice text NOT NULL,
    contact_hash text NOT NULL,
    counter_notice text,
    created_at timestamptz NOT NULL DEFAULT now(),
    reviewed_at timestamptz,
    counter_received_at timestamptz,
    restore_after timestamptz,
    restore_by timestamptz,
    forward_mail_id text,
    forward_state text,
    reason text,
    reviewer_id text REFERENCES users(id) ON DELETE SET NULL
);
ALTER TABLE reports DROP CONSTRAINT IF EXISTS reports_target_type_check;
ALTER TABLE reports ADD CONSTRAINT reports_target_type_check CHECK (target_type IN ('profile','wall_post','wall_reply','fan_art','setup_photo','chat_message','live_stream','emote','faction_post','guild','guild_emblem','vod','highlight','clip'));
