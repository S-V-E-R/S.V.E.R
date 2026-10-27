-- A go-live post in a streamer's server is removed when the stream ends, so the channel only
-- shows who is live now (as the legacy bot did).
ALTER TABLE discord_posts ADD COLUMN post_channel text, ADD COLUMN message_id text;
CREATE INDEX discord_posts_posted ON discord_posts(broadcast_id) WHERE message_id IS NOT NULL;
