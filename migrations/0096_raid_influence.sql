-- Raid influence (docs/FACTIONS.md "Raids"): a counted raid into a faction member's channel earns
-- the raiding streamer influence under the usual rules, a daily raid cap, and faction events.
ALTER TABLE faction_influence DROP CONSTRAINT faction_influence_source_check;
ALTER TABLE faction_influence ADD CONSTRAINT faction_influence_source_check
    CHECK (source IN ('stream', 'watch', 'chat', 'support', 'raid'));
