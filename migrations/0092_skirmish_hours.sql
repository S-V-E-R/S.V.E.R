-- Faction events, first event (docs/FACTIONS.md "Skirmish Hour"; Joe's decision October 10, 2026):
-- a staff-scheduled hour in which influence counts double, still under every per-person cap.
CREATE TABLE faction_skirmishes (
    id text PRIMARY KEY,
    starts_at timestamptz NOT NULL,
    created_by text REFERENCES users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    cancelled_at timestamptz
);
-- At most one scheduled Skirmish Hour starts at a time; overlaps are refused by the API.
CREATE UNIQUE INDEX faction_skirmishes_start ON faction_skirmishes (starts_at) WHERE cancelled_at IS NULL;
