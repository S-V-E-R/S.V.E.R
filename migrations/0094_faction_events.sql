-- More faction events (docs/FACTIONS.md "Faction events"; Joe's decision October 10, 2026), all 2x
-- under the usual per-person caps and never stacking:
--  * Raid Night: a staff-scheduled 3-hour window; viewers who arrived by a cross-faction raid
--    earn double watch influence. Stored beside Skirmish Hours with a kind.
--  * Territory Spotlight: one genre a UTC day, the week's closest race, picked on first use.
ALTER TABLE faction_skirmishes ADD COLUMN kind text NOT NULL DEFAULT 'skirmish' CHECK (kind IN ('skirmish', 'raid_night'));
DROP INDEX faction_skirmishes_start;
CREATE UNIQUE INDEX faction_skirmishes_start ON faction_skirmishes (kind, starts_at) WHERE cancelled_at IS NULL;
CREATE TABLE faction_spotlights (
    day date PRIMARY KEY,
    genre text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
