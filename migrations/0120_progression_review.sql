-- The "follow a new channel" order counts first follows only (an unfollow and refollow never counts
-- again). Existing follow bonuses predate this column and stay NULL: already followed.
ALTER TABLE engagement ADD COLUMN first_followed_at timestamptz;
CREATE INDEX engagement_first_followed ON engagement(user_id, first_followed_at) WHERE first_followed_at IS NOT NULL;

-- Staff decisions on Good Works submissions notify the streamer.
ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check CHECK(kind IN ('live','guild_application','guild_decision','guild_invite','squad_invite','sub_ending','hook_disabled','dm','light','mod_review','good_works'));
