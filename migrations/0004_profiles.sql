-- Module 2: Profiles (docs/PROFILES.md, "Data model"). Users, identities and legacy data are unchanged.
-- Profile rows are created on first write or by the legacy profile import; reads fall back to defaults.
CREATE TABLE profiles (
    user_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    display_name text NOT NULL,
    bio text NOT NULL DEFAULT '',
    mood_emoji text NOT NULL DEFAULT '',
    status_text text NOT NULL DEFAULT '',
    avatar_key text,
    banner_key text,
    internal boolean NOT NULL DEFAULT false,
    -- Cache of the effective restriction from active strikes and interim restrictions.
    restricted_until timestamptz,
    who_can_post text NOT NULL DEFAULT 'ANYONE' CHECK (who_can_post IN ('ANYONE', 'FOLLOWING', 'MUTUAL', 'NONE')),
    require_approval boolean NOT NULL DEFAULT false,
    hold_links boolean NOT NULL DEFAULT false,
    hold_new_accounts boolean NOT NULL DEFAULT false,
    fan_art_enabled boolean NOT NULL DEFAULT false,
    song_provider text CHECK (song_provider IN ('youtube', 'soundcloud')),
    song_media_id text,
    song_url text,
    song_title text,
    song_artist text,
    song_thumb_key text,
    song_volume integer NOT NULL DEFAULT 70 CHECK (song_volume BETWEEN 0 AND 100),
    song_notice text,
    song_updated_at timestamptz,
    follower_count integer NOT NULL DEFAULT 0,
    following_count integer NOT NULL DEFAULT 0,
    email_report_updates boolean NOT NULL DEFAULT false,
    report_digest_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    CHECK (song_provider IS NULL OR (song_media_id IS NOT NULL AND song_url IS NOT NULL))
);
CREATE INDEX profiles_display_name ON profiles (lower(display_name));

-- Per-section revision numbers for stale-edit detection (409 on a stale save).
CREATE TABLE profile_sections (
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    section text NOT NULL,
    revision bigint NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, section)
);

CREATE TABLE username_history (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    old_username text NOT NULL,
    new_username text NOT NULL,
    reason text NOT NULL CHECK (reason IN ('rename', 'revert', 'import')),
    changed_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX username_history_user ON username_history (user_id, changed_at DESC);

CREATE TABLE username_holds (
    handle_canonical text PRIMARY KEY CHECK (handle_canonical = lower(handle_canonical)),
    user_id text REFERENCES users(id) ON DELETE SET NULL,
    released_at timestamptz NOT NULL,
    redirect boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX username_holds_user ON username_holds (user_id);

CREATE TABLE social_links (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    position integer NOT NULL CHECK (position BETWEEN 1 AND 5),
    platform text NOT NULL,
    url text NOT NULL CHECK (length(url) <= 2048),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (user_id, position)
);

CREATE TABLE follows (
    follower_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    following_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (follower_id, following_id),
    CHECK (follower_id <> following_id)
);
CREATE INDEX follows_following ON follows (following_id, created_at DESC);
CREATE INDEX follows_follower ON follows (follower_id, created_at DESC);

CREATE TABLE user_blocks (
    blocker_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    blocked_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (blocker_id, blocked_id),
    CHECK (blocker_id <> blocked_id)
);
CREATE INDEX user_blocks_blocked ON user_blocks (blocked_id);

CREATE TABLE war_council (
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    position integer NOT NULL CHECK (position BETWEEN 1 AND 8),
    member_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (user_id, position),
    UNIQUE (user_id, member_id),
    CHECK (member_id <> user_id)
);
CREATE INDEX war_council_member ON war_council (member_id);

CREATE TABLE wall_posts (
    id text PRIMARY KEY,
    wall_owner_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    author_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body text NOT NULL,
    status text NOT NULL CHECK (status IN ('APPROVED', 'PENDING', 'REJECTED', 'REMOVED')),
    pinned_position integer CHECK (pinned_position BETWEEN 1 AND 3),
    created_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    moderated_at timestamptz,
    UNIQUE (wall_owner_id, pinned_position)
);
CREATE INDEX wall_posts_wall ON wall_posts (wall_owner_id, created_at DESC, id DESC);
CREATE INDEX wall_posts_author ON wall_posts (author_id);

CREATE TABLE wall_replies (
    id text PRIMARY KEY,
    post_id text NOT NULL REFERENCES wall_posts(id) ON DELETE CASCADE,
    author_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body text NOT NULL,
    status text NOT NULL CHECK (status IN ('APPROVED', 'PENDING', 'REJECTED', 'REMOVED')),
    created_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    moderated_at timestamptz
);
CREATE INDEX wall_replies_post ON wall_replies (post_id, created_at, id);
CREATE INDEX wall_replies_author ON wall_replies (author_id);

CREATE TABLE wall_likes (
    post_id text NOT NULL REFERENCES wall_posts(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (post_id, user_id)
);

CREATE TABLE schedules (
    user_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    timezone text NOT NULL
);
CREATE TABLE schedule_blocks (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    position integer NOT NULL,
    weekday integer NOT NULL CHECK (weekday BETWEEN 1 AND 7),
    start_minute integer NOT NULL CHECK (start_minute BETWEEN 0 AND 1439),
    end_minute integer NOT NULL CHECK (end_minute BETWEEN 0 AND 1439),
    label text NOT NULL DEFAULT '',
    CHECK (start_minute <> end_minute)
);
CREATE INDEX schedule_blocks_user ON schedule_blocks (user_id, position);
CREATE TABLE schedule_events (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title text NOT NULL,
    start_at timestamptz NOT NULL,
    end_at timestamptz NOT NULL,
    CHECK (end_at > start_at AND end_at <= start_at + interval '24 hours')
);
CREATE INDEX schedule_events_user ON schedule_events (user_id, start_at);

CREATE TABLE sponsors (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    position integer NOT NULL,
    active boolean NOT NULL DEFAULT true,
    name text NOT NULL,
    description text NOT NULL DEFAULT '',
    link text NOT NULL,
    discount_code text NOT NULL DEFAULT '',
    category text NOT NULL CHECK (category IN ('HARDWARE', 'PERIPHERALS', 'SOFTWARE', 'APPAREL', 'FOOD_DRINK', 'SERVICES', 'OTHER')),
    logo_key text
);
CREATE INDEX sponsors_user ON sponsors (user_id, position);

CREATE TABLE setup_items (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    position integer NOT NULL,
    category text NOT NULL CHECK (category IN ('CAMERA', 'MICROPHONE', 'AUDIO_INTERFACE', 'HEADPHONES', 'PC', 'CPU', 'GPU', 'CAPTURE', 'LIGHTING', 'MONITOR', 'KEYBOARD', 'MOUSE', 'CONTROLLER', 'OTHER')),
    name text NOT NULL,
    note text NOT NULL DEFAULT '',
    link text
);
CREATE INDEX setup_items_user ON setup_items (user_id, position);

CREATE TABLE profile_blocks (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    position integer NOT NULL,
    type text NOT NULL CHECK (type IN ('ABOUT', 'PANEL', 'QUOTES', 'GAME_SHELF')),
    enabled boolean NOT NULL DEFAULT true,
    config jsonb NOT NULL
);
CREATE INDEX profile_blocks_user ON profile_blocks (user_id, position);

CREATE TABLE fan_art (
    id text PRIMARY KEY,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    submitter_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    image_key text NOT NULL,
    artist_name text NOT NULL,
    artist_link text,
    caption text NOT NULL DEFAULT '',
    status text NOT NULL CHECK (status IN ('PENDING', 'APPROVED', 'REJECTED', 'REMOVED')),
    submitted_at timestamptz NOT NULL DEFAULT now(),
    reviewed_at timestamptz
);
CREATE INDEX fan_art_channel ON fan_art (channel_id, status, submitted_at DESC);
CREATE INDEX fan_art_submitter ON fan_art (submitter_id, submitted_at DESC);

CREATE TABLE reports (
    id text PRIMARY KEY,
    reporter_id text REFERENCES users(id) ON DELETE SET NULL,
    target_type text NOT NULL CHECK (target_type IN ('profile', 'wall_post', 'wall_reply', 'fan_art')),
    target_id text NOT NULL,
    -- Owner of the reported content; no foreign key so the report outlives an erased account.
    target_user_id text NOT NULL,
    target_username text NOT NULL,
    field text,
    reason text NOT NULL CHECK (reason IN ('spam', 'harassment', 'hate', 'sexual', 'violence', 'impersonation', 'private_information', 'copyright', 'other')),
    note text NOT NULL DEFAULT '',
    snapshot jsonb NOT NULL,
    status text NOT NULL DEFAULT 'OPEN' CHECK (status IN ('OPEN', 'ACTIONED', 'DISMISSED')),
    created_at timestamptz NOT NULL DEFAULT now(),
    closed_at timestamptz,
    closed_reason text,
    reporter_notice text NOT NULL DEFAULT 'NONE' CHECK (reporter_notice IN ('NONE', 'ACTION_TAKEN')),
    reporter_seen_at timestamptz,
    reporter_emailed boolean NOT NULL DEFAULT false
);
CREATE UNIQUE INDEX reports_one_open ON reports (reporter_id, target_type, target_id) WHERE status = 'OPEN';
CREATE INDEX reports_open ON reports (status, target_type, target_id, created_at);
CREATE INDEX reports_reporter ON reports (reporter_id, created_at DESC);

CREATE TABLE interim_restrictions (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    starts_at timestamptz NOT NULL DEFAULT now(),
    until timestamptz NOT NULL,
    created_by text NOT NULL,
    note text NOT NULL DEFAULT '',
    resolution text NOT NULL DEFAULT 'OPEN' CHECK (resolution IN ('OPEN', 'CONVERTED', 'LIFTED')),
    resolved_at timestamptz,
    resolved_by text,
    overdue_at timestamptz,
    CHECK (until > starts_at AND until <= starts_at + interval '24 hours')
);
CREATE INDEX interim_restrictions_open ON interim_restrictions (resolution, until);
CREATE UNIQUE INDEX interim_restrictions_one_open ON interim_restrictions (user_id) WHERE resolution = 'OPEN';

CREATE TABLE strikes (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    reason text NOT NULL,
    severity text NOT NULL CHECK (severity IN ('STANDARD', 'SEVERE')),
    content_snapshot jsonb NOT NULL,
    report_ids text[] NOT NULL DEFAULT '{}',
    removed_refs jsonb NOT NULL DEFAULT '[]',
    penalty text NOT NULL CHECK (penalty IN ('WARNING', 'RESTRICT_72H', 'RESTRICT_INDEFINITE')),
    interim_restriction_id text REFERENCES interim_restrictions(id) ON DELETE SET NULL,
    penalty_starts_at timestamptz NOT NULL,
    penalty_until timestamptz,
    -- Set when staff lift the restriction directly without overturning the strike.
    penalty_lifted_at timestamptz,
    level integer NOT NULL CHECK (level BETWEEN 1 AND 3),
    message_to_user text NOT NULL DEFAULT '',
    staff_note text NOT NULL DEFAULT '',
    issued_by text NOT NULL,
    issued_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    status text NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE', 'OVERTURNED')),
    acknowledged_at timestamptz,
    overturned_at timestamptz,
    ban_review_open boolean NOT NULL DEFAULT false,
    CHECK (penalty <> 'RESTRICT_72H' OR penalty_until = penalty_starts_at + interval '72 hours')
);
CREATE INDEX strikes_user ON strikes (user_id, issued_at DESC);

CREATE TABLE appeals (
    id text PRIMARY KEY,
    strike_id text NOT NULL UNIQUE REFERENCES strikes(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    body text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    status text NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING', 'UPHELD', 'OVERTURNED')),
    reviewed_by text,
    reviewed_at timestamptz,
    message_to_user text NOT NULL DEFAULT '',
    staff_note text NOT NULL DEFAULT '',
    self_review boolean NOT NULL DEFAULT false
);
CREATE INDEX appeals_pending ON appeals (status, created_at);

CREATE TABLE staff_roles (
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role text NOT NULL CHECK (role IN ('admin')),
    granted_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, role)
);

-- The audit record; never exposed outside /admin and kept after erasure.
CREATE TABLE moderation_actions (
    id text PRIMARY KEY,
    actor_id text,
    action text NOT NULL,
    target_type text NOT NULL,
    target_id text NOT NULL,
    report_ids text[] NOT NULL DEFAULT '{}',
    note text NOT NULL DEFAULT '',
    detail jsonb NOT NULL DEFAULT '{}',
    self_review boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX moderation_actions_target ON moderation_actions (target_type, target_id, created_at DESC);
CREATE INDEX moderation_actions_recent ON moderation_actions (created_at DESC, id DESC);

CREATE TABLE media_objects (
    key text PRIMARY KEY,
    owner_id text REFERENCES users(id) ON DELETE SET NULL,
    kind text NOT NULL,
    bytes bigint NOT NULL,
    width integer,
    height integer,
    created_at timestamptz NOT NULL DEFAULT now(),
    delete_after timestamptz
);
CREATE INDEX media_objects_cleanup ON media_objects (delete_after) WHERE delete_after IS NOT NULL;

CREATE TABLE import_runs (
    name text PRIMARY KEY,
    completed_at timestamptz NOT NULL DEFAULT now(),
    counts jsonb NOT NULL
);

-- Shared read model: display fields with defaults for users without a profile row, and the
-- channel eligibility rule (not internal, deleted, held or restricted).
CREATE VIEW channel_users AS
SELECT u.id,
       u.username,
       u.created_at,
       u.email_verified,
       u.deleted_at,
       coalesce(p.display_name, u.username) AS display_name,
       coalesce(p.bio, '') AS bio,
       coalesce(p.mood_emoji, '') AS mood_emoji,
       coalesce(p.status_text, '') AS status_text,
       p.avatar_key,
       p.banner_key,
       (coalesce(p.internal, false) OR lower(u.username) IN ('admin', 'support', 'sver')) AS internal,
       (p.restricted_until IS NOT NULL AND p.restricted_until > now()) AS restricted,
       p.restricted_until,
       (u.deleted_at IS NULL
        AND NOT (coalesce(p.internal, false) OR lower(u.username) IN ('admin', 'support', 'sver'))
        AND NOT (p.restricted_until IS NOT NULL AND p.restricted_until > now())) AS eligible
FROM users u
LEFT JOIN profiles p ON p.user_id = u.id;
