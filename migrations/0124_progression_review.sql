-- The "follow a new channel" order counts first follows only (an unfollow and refollow never counts
-- again). Existing follow bonuses predate this column and stay NULL: already followed.
ALTER TABLE engagement ADD COLUMN first_followed_at timestamptz;
CREATE INDEX engagement_first_followed ON engagement(user_id, first_followed_at) WHERE first_followed_at IS NOT NULL;
