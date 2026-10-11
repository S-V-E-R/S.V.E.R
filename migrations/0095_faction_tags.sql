-- Faction-tagged clips and Beacons (docs/BEACONS.md "Faction tab"; Joe's decision October 10, 2026).
-- Each clip and Beacon keeps the faction its channel belonged to when it was made, so a later
-- faction switch doesn't move old work. Set by a trigger, so every way of creating one is covered.
ALTER TABLE beacons ADD COLUMN faction text CHECK (faction IN ('myria', 'aetheron', 'glint'));
ALTER TABLE videos ADD COLUMN faction text CHECK (faction IN ('myria', 'aetheron', 'glint'));
CREATE FUNCTION tag_faction() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.faction IS NULL AND NEW.owner_id IS NOT NULL THEN
        NEW.faction := (SELECT faction FROM faction_members WHERE user_id = NEW.owner_id);
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER beacons_faction BEFORE INSERT ON beacons FOR EACH ROW EXECUTE FUNCTION tag_faction();
CREATE TRIGGER videos_faction BEFORE INSERT ON videos FOR EACH ROW EXECUTE FUNCTION tag_faction();
-- Existing work takes its channel's current faction.
UPDATE beacons b SET faction = m.faction FROM faction_members m WHERE m.user_id = b.owner_id;
UPDATE videos v SET faction = m.faction FROM faction_members m WHERE m.user_id = v.owner_id AND v.kind = 'CLIP';
CREATE INDEX beacons_faction ON beacons (faction, published_at DESC) WHERE faction IS NOT NULL;
