-- MAGNet review fixes (docs/MAGNET.md, October 16, 2026).

-- When a stream's category last changed: genre lanes take a stream only once its category has
-- settled, so hopping categories can't collect a first-time turn on every genre lane.
ALTER TABLE stream_settings ADD COLUMN category_changed_at timestamptz;
CREATE FUNCTION stream_category_changed() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.category_id IS DISTINCT FROM OLD.category_id THEN
        NEW.category_changed_at := now();
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER stream_settings_category_changed BEFORE UPDATE OF category_id ON stream_settings
    FOR EACH ROW EXECUTE FUNCTION stream_category_changed();

-- The first time each account followed each channel. Unfollowing deletes the follow but not this,
-- so unfollowing and following again can't fake a burst of new follows for MAGNet.
CREATE TABLE follow_firsts (
    follower_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    following_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    first_at timestamptz NOT NULL,
    PRIMARY KEY (follower_id, following_id)
);
CREATE INDEX follow_firsts_following ON follow_firsts (following_id, first_at DESC);
INSERT INTO follow_firsts(follower_id, following_id, first_at)
    SELECT follower_id, following_id, created_at FROM follows ON CONFLICT DO NOTHING;
CREATE FUNCTION follow_first() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO follow_firsts(follower_id, following_id, first_at)
        VALUES (NEW.follower_id, NEW.following_id, NEW.created_at) ON CONFLICT DO NOTHING;
    RETURN NEW;
END $$;
CREATE TRIGGER follows_first AFTER INSERT ON follows FOR EACH ROW EXECUTE FUNCTION follow_first();
