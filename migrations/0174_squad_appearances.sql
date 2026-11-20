-- Co-stream history for chat replay (docs/VODS_CLIPS.md "Chat replay"). squad_members rows are
-- deleted when someone leaves, so each stay is kept here: a guest's VOD and clips replay the shared
-- chat from the time they were in the co-stream, and the host's no longer mix in lines from outside it.
-- Triggers record every join and leave, including repairs and cascades.
CREATE TABLE squad_appearances (
    squad_id text NOT NULL REFERENCES squads(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    broadcast_id text REFERENCES broadcasts(id) ON DELETE SET NULL,
    joined_at timestamptz NOT NULL,
    left_at timestamptz,
    PRIMARY KEY (user_id, squad_id, joined_at)
);

CREATE FUNCTION squad_appearance_join() RETURNS trigger AS $$
BEGIN
    INSERT INTO squad_appearances(squad_id,user_id,broadcast_id,joined_at)
    VALUES (NEW.squad_id,NEW.user_id,NEW.broadcast_id,NEW.joined_at) ON CONFLICT DO NOTHING;
    RETURN NULL;
END $$ LANGUAGE plpgsql;
CREATE TRIGGER squad_appearance_join AFTER INSERT ON squad_members
    FOR EACH ROW EXECUTE FUNCTION squad_appearance_join();

CREATE FUNCTION squad_appearance_leave() RETURNS trigger AS $$
BEGIN
    UPDATE squad_appearances SET left_at=now()
    WHERE squad_id=OLD.squad_id AND user_id=OLD.user_id AND left_at IS NULL;
    RETURN NULL;
END $$ LANGUAGE plpgsql;
CREATE TRIGGER squad_appearance_leave AFTER DELETE ON squad_members
    FOR EACH ROW EXECUTE FUNCTION squad_appearance_leave();

-- Live co-streams keep every member; past ones only kept their host, who keeps the shared chat
-- they always replayed. Guests of past co-streams can't be recovered.
INSERT INTO squad_appearances(squad_id,user_id,broadcast_id,joined_at)
SELECT squad_id,user_id,broadcast_id,joined_at FROM squad_members;
INSERT INTO squad_appearances(squad_id,user_id,joined_at,left_at)
SELECT s.id,s.host_id,s.created_at,coalesce(s.ended_at,now()) FROM squads s
WHERE s.host_id IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM squad_members m WHERE m.squad_id=s.id AND m.user_id=s.host_id)
ON CONFLICT DO NOTHING;
