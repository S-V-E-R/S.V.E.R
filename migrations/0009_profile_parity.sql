-- Module 2 parity additions P1-P9 (docs/PROFILES.md, "Parity additions").
ALTER TABLE profiles
    ADD COLUMN show_linked_accounts boolean NOT NULL DEFAULT false,
    ADD COLUMN readiness_dismissed_at timestamptz,
    ADD COLUMN setup_title text NOT NULL DEFAULT '',
    ADD COLUMN setup_description text NOT NULL DEFAULT '',
    ADD COLUMN page_label text NOT NULL DEFAULT '',
    ADD COLUMN welcome_line text NOT NULL DEFAULT '',
    ADD COLUMN intro_title text NOT NULL DEFAULT '',
    ADD COLUMN intro_body text NOT NULL DEFAULT '',
    ADD COLUMN page_vibe text NOT NULL DEFAULT '',
    ADD COLUMN header_copy_enabled boolean NOT NULL DEFAULT true;

-- Public provider handle (Twitch login, Discord username) captured at sign-in; never a secret.
ALTER TABLE identities ADD COLUMN handle text;

CREATE TABLE setup_photos (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    position integer NOT NULL CHECK (position BETWEEN 0 AND 2),
    image_key text NOT NULL,
    alt text NOT NULL DEFAULT '',
    status text NOT NULL DEFAULT 'VISIBLE' CHECK (status IN ('VISIBLE', 'REMOVED')),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX setup_photos_user ON setup_photos (user_id, position);

-- Kinds are validated by the registry in activity.rs so later modules add kinds without a migration.
CREATE TABLE activity_events (
    id text PRIMARY KEY,
    actor_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind text NOT NULL,
    subject_id text REFERENCES users(id) ON DELETE CASCADE,
    ref_id text,
    data jsonb NOT NULL DEFAULT '{}'::jsonb,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX activity_actor ON activity_events (actor_id, created_at DESC, id DESC);
CREATE INDEX activity_subject ON activity_events (subject_id);
CREATE UNIQUE INDEX activity_one_follow ON activity_events (actor_id, subject_id) WHERE kind = 'follow';

ALTER TABLE reports DROP CONSTRAINT reports_target_type_check;
ALTER TABLE reports ADD CONSTRAINT reports_target_type_check
    CHECK (target_type IN ('profile', 'wall_post', 'wall_reply', 'fan_art', 'setup_photo'));
