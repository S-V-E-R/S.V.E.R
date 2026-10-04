CREATE TABLE channel_emotes (
    id text PRIMARY KEY,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code text COLLATE "C" NOT NULL CHECK (code ~ '^[A-Za-z0-9]{3,20}$'),
    image_key text NOT NULL,
    status text NOT NULL DEFAULT 'VISIBLE' CHECK (status IN ('VISIBLE','REMOVED')),
    reviewed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (channel_id, code)
);
CREATE INDEX channel_emotes_image ON channel_emotes(image_key);
CREATE INDEX channel_emotes_review ON channel_emotes(created_at) WHERE reviewed_at IS NULL;

-- Also covers account deletion. Shared content remains protected by the media cleanup check.
CREATE FUNCTION release_emote_media() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    UPDATE media_objects SET delete_after=now() WHERE key LIKE OLD.image_key || '/%'
      AND NOT EXISTS (SELECT 1 FROM channel_emotes WHERE image_key=OLD.image_key);
    RETURN OLD;
END;
$$;
CREATE TRIGGER release_emote_media AFTER DELETE ON channel_emotes
FOR EACH ROW EXECUTE FUNCTION release_emote_media();

ALTER TABLE reports DROP CONSTRAINT reports_target_type_check;
ALTER TABLE reports ADD CONSTRAINT reports_target_type_check
    CHECK (target_type IN ('profile','wall_post','wall_reply','fan_art','setup_photo','chat_message','live_stream','emote'));
