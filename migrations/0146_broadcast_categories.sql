-- Raven's Eye stream analytics (docs/ADMIN.md): which category each broadcast was in, and when, so
-- hours count toward the game that was actually streamed even after the channel changes category.
-- Triggers keep it, so every writer of a broadcast or a channel's category is covered.
CREATE TABLE broadcast_categories (
    broadcast_id text NOT NULL REFERENCES broadcasts(id) ON DELETE CASCADE,
    -- No foreign key: categories can be merged or renamed; the name is kept as it was streamed.
    category_id text,
    name text NOT NULL,
    genre text,
    started_at timestamptz NOT NULL,
    ended_at timestamptz,
    PRIMARY KEY (broadcast_id, started_at)
);
CREATE INDEX broadcast_categories_time ON broadcast_categories (started_at);

CREATE FUNCTION broadcast_category_open(broadcast text, owner text, at timestamptz) RETURNS void AS $$
    INSERT INTO broadcast_categories(broadcast_id,category_id,name,genre,started_at)
    SELECT broadcast,c.id,coalesce(c.name,'Uncategorized'),c.genre,at
    FROM (SELECT 1) one LEFT JOIN stream_settings s ON s.owner_id=owner LEFT JOIN stream_categories c ON c.id=s.category_id
    ON CONFLICT (broadcast_id,started_at) DO UPDATE SET category_id=EXCLUDED.category_id,name=EXCLUDED.name,genre=EXCLUDED.genre;
$$ LANGUAGE sql;

CREATE FUNCTION broadcast_categories_start() RETURNS trigger AS $$
BEGIN
    PERFORM broadcast_category_open(NEW.id, NEW.owner_id, NEW.started_at);
    RETURN NULL;
END $$ LANGUAGE plpgsql;
CREATE TRIGGER broadcast_categories_start AFTER INSERT ON broadcasts
    FOR EACH ROW EXECUTE FUNCTION broadcast_categories_start();

CREATE FUNCTION broadcast_categories_end() RETURNS trigger AS $$
BEGIN
    UPDATE broadcast_categories SET ended_at=NEW.ended_at WHERE broadcast_id=NEW.id AND ended_at IS NULL;
    RETURN NULL;
END $$ LANGUAGE plpgsql;
CREATE TRIGGER broadcast_categories_end AFTER UPDATE OF ended_at ON broadcasts
    FOR EACH ROW WHEN (OLD.ended_at IS NULL AND NEW.ended_at IS NOT NULL) EXECUTE FUNCTION broadcast_categories_end();

-- A category change while live closes the open span and opens one in the new category.
CREATE FUNCTION broadcast_categories_change() RETURNS trigger AS $$
DECLARE live text;
BEGIN
    SELECT id INTO live FROM broadcasts WHERE owner_id=NEW.owner_id AND ended_at IS NULL;
    IF live IS NOT NULL THEN
        UPDATE broadcast_categories SET ended_at=now() WHERE broadcast_id=live AND ended_at IS NULL AND started_at<now();
        PERFORM broadcast_category_open(live, NEW.owner_id, now());
    END IF;
    RETURN NULL;
END $$ LANGUAGE plpgsql;
CREATE TRIGGER broadcast_categories_change AFTER UPDATE OF category_id ON stream_settings
    FOR EACH ROW WHEN (OLD.category_id IS DISTINCT FROM NEW.category_id) EXECUTE FUNCTION broadcast_categories_change();

-- Past broadcasts get one span, in the category Raven's Eye used for them until now: the
-- recording's, else the channel's current one.
INSERT INTO broadcast_categories(broadcast_id,category_id,name,genre,started_at,ended_at)
SELECT b.id,CASE WHEN v.category IS NULL THEN c.id ELSE v.category_id END,coalesce(v.category,c.name,'Uncategorized'),
    CASE WHEN v.category IS NULL THEN c.genre ELSE vc.genre END,b.started_at,b.ended_at
FROM broadcasts b
LEFT JOIN LATERAL (SELECT category_id,category FROM videos WHERE broadcast_id=b.id AND kind='VOD' AND category IS NOT NULL LIMIT 1) v ON true
LEFT JOIN stream_categories vc ON vc.id=v.category_id
LEFT JOIN stream_settings s ON s.owner_id=b.owner_id LEFT JOIN stream_categories c ON c.id=s.category_id;
