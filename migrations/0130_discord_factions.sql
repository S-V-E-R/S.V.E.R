-- Discord factions and S.V.E.R sync on the home server (docs/DISCORD_BOT.md "Factions in Discord",
-- "Account link and sync"): Discord chat and voice as war influence, seasons, holidays, territory
-- posts, role and level sync for linked accounts, and the Discord rank on S.V.E.R profiles.
ALTER TABLE faction_influence DROP CONSTRAINT faction_influence_source_check;
ALTER TABLE faction_influence ADD CONSTRAINT faction_influence_source_check
    CHECK (source IN ('stream', 'watch', 'chat', 'support', 'raid', 'discord'));

-- The S.V.E.R level last seen for a linked member (a rise is announced), and the optional nickname
-- sync (`/link nickname`).
ALTER TABLE discord_members ADD COLUMN sver_level integer,
    ADD COLUMN nickname boolean NOT NULL DEFAULT false;

-- Discord faction-point seasons; at most one running.
CREATE TABLE discord_seasons (
    number integer PRIMARY KEY CHECK (number > 0),
    name text NOT NULL,
    starts_at timestamptz NOT NULL DEFAULT now(),
    ends_at timestamptz NOT NULL,
    winner text CHECK (winner IN ('myria', 'aetheron', 'glint')),
    ended_at timestamptz,
    CHECK (starts_at < ends_at)
);
CREATE UNIQUE INDEX discord_seasons_one_active ON discord_seasons((true)) WHERE ended_at IS NULL;

-- Faction holidays (UTC dates): a faction multiplier for the day and points for every member.
CREATE TABLE discord_holidays (
    id bigserial PRIMARY KEY,
    faction text NOT NULL CHECK (faction IN ('myria', 'aetheron', 'glint')),
    name text NOT NULL UNIQUE,
    month smallint NOT NULL CHECK (month BETWEEN 1 AND 12),
    day smallint NOT NULL CHECK (day BETWEEN 1 AND 31),
    factor numeric(4, 2) NOT NULL CHECK (factor > 0 AND factor <= 10),
    points integer NOT NULL DEFAULT 0 CHECK (points >= 0),
    active boolean NOT NULL DEFAULT true,
    last_run date
);
INSERT INTO discord_holidays(faction, name, month, day, factor, points) VALUES
    ('myria', 'Myria Founding Day', 3, 15, 2, 25),
    ('myria', 'The Iron Trial', 9, 20, 1.5, 15),
    ('aetheron', 'Aetheron Shield Day', 5, 10, 2, 25),
    ('aetheron', 'The Vigil', 11, 11, 1.5, 15),
    ('glint', 'Glint Spark Festival', 7, 4, 2, 25),
    ('glint', 'Winter Luminance', 12, 21, 1.5, 15);

-- Territory holders last posted, per S.V.E.R season, so a change is posted once.
CREATE TABLE discord_territory_seen (
    genre text PRIMARY KEY,
    season_id bigint NOT NULL,
    holder text
);

-- S.V.E.R level bands: a linked member gets the role of the highest band at or below their level.
CREATE TABLE discord_level_roles (
    level integer PRIMARY KEY CHECK (level > 0),
    role text NOT NULL UNIQUE
);

-- Roles the sync gave on the home server; only these are ever taken back.
CREATE TABLE discord_home_grants (
    discord_user text NOT NULL,
    role text NOT NULL,
    PRIMARY KEY (discord_user, role)
);

-- Owners who show their Discord rank on their S.V.E.R profile.
CREATE TABLE discord_rank_shown (
    user_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE
);
