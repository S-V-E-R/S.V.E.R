-- Historical eligibility must survive a broadcaster reconnect clearing its latest health probe.
ALTER TABLE broadcasts ADD COLUMN confirmed_live_at timestamptz;
UPDATE broadcasts SET confirmed_live_at=coalesce(observed_at,started_at)
WHERE state='LIVE' OR alert_state='sent' OR
 (observed_at IS NOT NULL AND lower(health->>'video_codec') IN ('h264','avc') AND lower(health->>'audio_codec')='aac');
CREATE TABLE guilds (
    id text PRIMARY KEY,
    slug text NOT NULL,
    name text NOT NULL CHECK (char_length(name) BETWEEN 3 AND 40),
    tag text NOT NULL CHECK (char_length(tag) BETWEEN 2 AND 5),
    tagline text NOT NULL DEFAULT '' CHECK (char_length(tagline)<=120),
    about text NOT NULL DEFAULT '' CHECK (char_length(about)<=3000),
    leader_id text REFERENCES users(id) ON DELETE SET NULL,
    status text NOT NULL DEFAULT 'ACTIVE' CHECK(status IN ('ACTIVE','ARCHIVED','REMOVED')),
    recruiting boolean NOT NULL DEFAULT true,
    verified boolean NOT NULL DEFAULT false,
    avatar_key text,
    banner_key text,
    emblem_visible boolean NOT NULL DEFAULT true,
    emblem_reviewed_at timestamptz,
    revision bigint NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX guild_slug ON guilds(lower(slug));
CREATE UNIQUE INDEX guild_name ON guilds(lower(name));
CREATE UNIQUE INDEX guild_tag ON guilds(lower(tag));
CREATE UNIQUE INDEX guild_one_leader ON guilds(leader_id) WHERE status='ACTIVE';
CREATE TABLE guild_members (
    guild_id text NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    officer boolean NOT NULL DEFAULT false,
    title text NOT NULL DEFAULT '' CHECK(char_length(title)<=40),
    joined_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(guild_id,user_id)
);
CREATE INDEX guild_members_user ON guild_members(user_id);
CREATE TABLE guild_badges (
    user_id text PRIMARY KEY,
    guild_id text NOT NULL,
    FOREIGN KEY(guild_id,user_id) REFERENCES guild_members(guild_id,user_id) ON DELETE CASCADE
);
CREATE TABLE guild_applications (
    id text PRIMARY KEY,
    guild_id text NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    message text NOT NULL CHECK(char_length(message) BETWEEN 1 AND 500),
    status text NOT NULL DEFAULT 'OPEN' CHECK(status IN ('OPEN','ACCEPTED','DECLINED','WITHDRAWN')),
    note text NOT NULL DEFAULT '' CHECK(char_length(note)<=500),
    created_at timestamptz NOT NULL DEFAULT now(),
    decided_at timestamptz,
    declined_at timestamptz,
    UNIQUE(guild_id,user_id)
);
CREATE TABLE guild_invitations (
    guild_id text NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    invited_by text REFERENCES users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(guild_id,user_id)
);
CREATE TABLE guild_blocks (
    guild_id text NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY(guild_id,user_id)
);
CREATE TABLE guild_follows (
    guild_id text NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY(guild_id,user_id)
);
CREATE INDEX guild_follows_user ON guild_follows(user_id);
CREATE TABLE guild_mutes (
    guild_id text NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY(guild_id,user_id)
);
CREATE TABLE guild_events (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    guild_id text NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    actor_id text REFERENCES users(id) ON DELETE SET NULL,
    subject_id text REFERENCES users(id) ON DELETE SET NULL,
    action text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX guild_events_recent ON guild_events(guild_id,id DESC);
CREATE TABLE guild_verification (
    guild_id text PRIMARY KEY REFERENCES guilds(id) ON DELETE CASCADE,
    evidence text NOT NULL CHECK(char_length(evidence) BETWEEN 1 AND 2000),
    status text NOT NULL DEFAULT 'OPEN' CHECK(status IN ('OPEN','APPROVED','DECLINED')),
    note text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE reports DROP CONSTRAINT reports_target_type_check;
ALTER TABLE reports ADD CONSTRAINT reports_target_type_check CHECK(target_type IN
    ('profile','wall_post','wall_reply','fan_art','setup_photo','chat_message','live_stream','emote','faction_post','guild','guild_emblem'));

ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check CHECK(kind IN ('live','guild_application','guild_decision','guild_invite','squad_invite'));
ALTER TABLE notifications ALTER COLUMN broadcast_id DROP NOT NULL;
ALTER TABLE notifications ADD COLUMN event_key text;
ALTER TABLE notifications ADD COLUMN payload jsonb NOT NULL DEFAULT '{}';
ALTER TABLE notifications ADD COLUMN guild_id text REFERENCES guilds(id) ON DELETE CASCADE;
ALTER TABLE notifications ADD COLUMN site_visible boolean NOT NULL DEFAULT true;
CREATE UNIQUE INDEX notification_event ON notifications(user_id,event_key);
CREATE TABLE notification_type_settings (
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind text NOT NULL CHECK(kind IN ('guild_application','guild_decision','guild_invite','squad_invite')),
    site boolean NOT NULL DEFAULT true,
    push boolean NOT NULL DEFAULT true,
    PRIMARY KEY(user_id,kind)
);
ALTER TABLE push_jobs ALTER COLUMN broadcast_id DROP NOT NULL;
ALTER TABLE push_jobs ADD COLUMN notification_id text REFERENCES notifications(id) ON DELETE CASCADE;
ALTER TABLE push_jobs ADD CONSTRAINT push_job_target CHECK((broadcast_id IS NULL)<>(notification_id IS NULL));
CREATE UNIQUE INDEX push_notification ON push_jobs(subscription_id,notification_id);
