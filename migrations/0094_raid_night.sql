-- Raid influence and Raid Night (docs/FACTIONS.md "Raids" and "Raid Night"; Joe chose Raid Night
-- as the next faction event, October 10, 2026).
-- A raid into a faction member's channel earns the raiding streamer influence (normal rules, caps).
ALTER TABLE faction_influence DROP CONSTRAINT faction_influence_source_check;
ALTER TABLE faction_influence ADD CONSTRAINT faction_influence_source_check
    CHECK (source IN ('stream', 'watch', 'chat', 'support', 'raid'));
-- Faction events share one schedule: a Skirmish Hour (1 hour) or a Raid Night (3 hours). Events
-- never overlap; the API refuses overlaps.
ALTER TABLE faction_skirmishes
    ADD COLUMN kind text NOT NULL DEFAULT 'skirmish' CHECK (kind IN ('skirmish', 'raid_night')),
    ADD COLUMN ends_at timestamptz;
UPDATE faction_skirmishes SET ends_at = starts_at + interval '1 hour';
ALTER TABLE faction_skirmishes ALTER COLUMN ends_at SET NOT NULL;
