-- Channel editors (docs/CHANNEL_ADDITIONS.md): up to 5 people per channel who can change the stream's
-- title, category and language and switch the Mature label on. Edits record who made them.
CREATE TABLE channel_editors (
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    appointed_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (channel_id, user_id),
    CHECK (channel_id <> user_id)
);
CREATE INDEX channel_editors_user ON channel_editors (user_id);
ALTER TABLE stream_settings ADD COLUMN edited_by text REFERENCES users(id) ON DELETE SET NULL;
