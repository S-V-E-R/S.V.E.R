-- A broadcast labeled mature at any point labels its VOD, highlights, clips and Beacons
-- (docs/CHANNEL_ADDITIONS.md), including ones made before the label went on mid-broadcast and
-- ones cut later from a copy that was saved unlabeled. (The recording hook also raises the VOD's
-- label at each segment, for a label switched on while the first segment was still saving.)
CREATE FUNCTION video_inherits_mature() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    NEW.mature := NEW.mature OR EXISTS(SELECT 1 FROM broadcasts WHERE id=NEW.broadcast_id AND mature);
    RETURN NEW;
END $$;
CREATE TRIGGER videos_inherit_mature BEFORE INSERT ON videos
    FOR EACH ROW WHEN (NEW.broadcast_id IS NOT NULL AND NOT NEW.mature)
    EXECUTE FUNCTION video_inherits_mature();

CREATE FUNCTION broadcast_labels_videos() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    UPDATE videos SET mature=true,revision=revision+1 WHERE broadcast_id=NEW.id AND NOT mature;
    UPDATE beacons SET mature=true WHERE broadcast_id=NEW.id AND NOT mature;
    RETURN NULL;
END $$;
CREATE TRIGGER broadcasts_label_videos AFTER UPDATE OF mature ON broadcasts
    FOR EACH ROW WHEN (NEW.mature AND NOT OLD.mature)
    EXECUTE FUNCTION broadcast_labels_videos();

UPDATE videos v SET mature=true,revision=v.revision+1
FROM broadcasts b WHERE b.id=v.broadcast_id AND b.mature AND NOT v.mature;
UPDATE beacons x SET mature=true
FROM broadcasts b WHERE b.id=x.broadcast_id AND b.mature AND NOT x.mature;
