-- Faction review fixes (docs/FACTIONS.md, October 16, 2026): an event that earned nothing because
-- a cap was reached is still recorded (0 points), so retrying it on another day can't earn.
ALTER TABLE faction_influence DROP CONSTRAINT faction_influence_points_check;
ALTER TABLE faction_influence ADD CONSTRAINT faction_influence_points_check CHECK (points >= 0);
