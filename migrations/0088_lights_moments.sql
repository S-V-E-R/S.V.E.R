-- Lights and Shine Moments (docs/SUPPORT.md "Shine").
CREATE TABLE shine_moments (
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    video_id text NOT NULL REFERENCES videos(id) ON DELETE CASCADE,
    position smallint NOT NULL,
    title text NOT NULL DEFAULT '',
    description text NOT NULL DEFAULT '',
    PRIMARY KEY (channel_id, video_id)
);
CREATE TABLE lights (
    id text PRIMARY KEY,
    from_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    to_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    message text CHECK (char_length(message) <= 200),
    hidden boolean NOT NULL DEFAULT false,
    day date NOT NULL DEFAULT (now() AT TIME ZONE 'UTC')::date,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (from_id, to_id, day),
    CHECK (from_id <> to_id)
);
CREATE INDEX lights_to ON lights (to_id, created_at DESC);
ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check CHECK(kind IN ('live','guild_application','guild_decision','guild_invite','squad_invite','sub_ending','hook_disabled','dm','light'));
