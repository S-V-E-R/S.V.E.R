-- Faction choice, brought forward from Module 4 (docs/FACTIONS.md, "Membership") on October 4, 2026.
-- The seasonal war, influence and the map still arrive with Module 4.
ALTER TABLE users ADD COLUMN faction text CHECK (faction IN ('myria', 'aetheron', 'glint'));
ALTER TABLE users ADD COLUMN faction_chosen_at timestamptz;

-- Every choice and switch is logged (FACTIONS.md: "Every switch is logged").
CREATE TABLE faction_changes (
    id bigserial PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    from_faction text CHECK (from_faction IN ('myria', 'aetheron', 'glint')),
    to_faction text NOT NULL CHECK (to_faction IN ('myria', 'aetheron', 'glint')),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX faction_changes_user ON faction_changes(user_id, created_at);
