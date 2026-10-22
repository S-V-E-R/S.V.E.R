-- Verified followers (docs/PROFILES.md): a follow counts once the follower has watched the
-- channel live for 15 minutes. Live watch time per channel comes from the engagement award.
ALTER TABLE engagement ADD COLUMN watch_minutes integer NOT NULL DEFAULT 0;
ALTER TABLE follows ADD COLUMN counted_at timestamptz;
-- Follows from before launch keep counting.
UPDATE follows SET counted_at=created_at;
CREATE INDEX follows_pending ON follows(following_id) WHERE counted_at IS NULL;
