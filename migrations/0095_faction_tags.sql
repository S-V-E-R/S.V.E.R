-- Faction-tagged Beacons (docs/BEACONS.md "Faction tab"; Joe's decision October 10, 2026). Clips
-- already carry the faction of the stream they came from (videos.faction, migration 0042); a
-- Beacon now keeps the faction its channel belonged to when it was made, so a later faction switch
-- doesn't move old work. Set by a trigger, so every way of creating one is covered.
ALTER TABLE beacons ADD COLUMN faction text CHECK (faction IN ('myria', 'aetheron', 'glint'));
CREATE FUNCTION tag_beacon_faction() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.faction IS NULL THEN
        NEW.faction := (SELECT faction FROM faction_members WHERE user_id = NEW.owner_id);
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER beacons_faction BEFORE INSERT ON beacons FOR EACH ROW EXECUTE FUNCTION tag_beacon_faction();
-- Existing Beacons take their channel's current faction.
UPDATE beacons b SET faction = m.faction FROM faction_members m WHERE m.user_id = b.owner_id;
CREATE INDEX beacons_faction ON beacons (faction, published_at DESC) WHERE faction IS NOT NULL;
