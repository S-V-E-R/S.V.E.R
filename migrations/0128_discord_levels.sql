-- Discord levels and ranks, multipliers, faction points, quests and achievements on the home
-- server (docs/DISCORD_BOT.md "Levels and ranks", "Quests and achievements"). Discord users are
-- keyed by their Discord id; a linked S.V.E.R account is found through `identities`.
CREATE TABLE discord_members (
    discord_user text PRIMARY KEY,
    faction text CHECK (faction IN ('myria', 'aetheron', 'glint')),
    joined_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE discord_xp (
    discord_user text PRIMARY KEY,
    total bigint NOT NULL DEFAULT 0 CHECK (total >= 0),
    -- The rank held (discord_ranks.rank); 0 before the first XP.
    rank smallint NOT NULL DEFAULT 0,
    messages integer NOT NULL DEFAULT 0,
    reactions integer NOT NULL DEFAULT 0,
    voice_minutes integer NOT NULL DEFAULT 0,
    watch_minutes integer NOT NULL DEFAULT 0,
    stream_minutes integer NOT NULL DEFAULT 0,
    -- The daily cap counts XP earned on `day` (UTC).
    day date NOT NULL DEFAULT current_date,
    day_xp integer NOT NULL DEFAULT 0,
    message_at timestamptz,
    reaction_at timestamptz,
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX discord_xp_total ON discord_xp(total DESC);
CREATE TABLE discord_ranks (
    rank smallint PRIMARY KEY CHECK (rank > 0),
    xp bigint NOT NULL UNIQUE CHECK (xp >= 0),
    base_name text NOT NULL,
    myria text NOT NULL,
    aetheron text NOT NULL,
    glint text NOT NULL,
    -- Discord role per variant: {"none": id, "myria": id, "aetheron": id, "glint": id}.
    roles jsonb NOT NULL DEFAULT '{}'
);
INSERT INTO discord_ranks(rank, xp, base_name, myria, aetheron, glint) VALUES
    (1, 0, 'Private', 'Initiate', 'Novice', 'Spark'),
    (2, 500, 'PFC', 'Sentinel-II', 'Seeker', 'Flicker'),
    (3, 1000, 'Specialist', 'Sentinel-I', 'Scholar', 'Kindle'),
    (4, 2000, 'Corporal', 'Warblade', 'Keeper', 'Torch'),
    (5, 4000, 'Sergeant', 'Sentinel', 'Warden', 'Ember'),
    (6, 7000, 'Staff Sgt', 'Iron Guard', 'High Warden', 'Blazeborn'),
    (7, 11000, 'SFC', 'Shield Wall', 'Sage', 'Firebrand'),
    (8, 16000, 'Master Sgt', 'Warmaster', 'Oracle', 'Pyroclast'),
    (9, 22000, 'First Sgt', 'Champion', 'Seer', 'Infernus'),
    (10, 30000, 'Sgt Major', 'Grand Champ', 'High Seer', 'Magmus'),
    (11, 40000, 'Cmd Sgt Maj', 'Warlord', 'Luminary', 'Vulcanis'),
    (12, 55000, '2nd Lieutenant', 'Blade Cmdr', 'Arcanist', 'Flamelord'),
    (13, 70000, '1st Lieutenant', 'War Cmdr', 'High Arc', 'Emberlord'),
    (14, 90000, 'Captain', 'Vanguard', 'Archon', 'Blaze'),
    (15, 115000, 'Major', 'High Van', 'Magistrate', 'Scorchwind'),
    (16, 145000, 'Lt Colonel', 'Battlemaster', 'Tribunal', 'Ashbringer'),
    (17, 180000, 'Colonel', 'Grand Battle', 'High Trib', 'Cataclysm'),
    (18, 225000, 'Brig General', 'Siege Lord', 'Consul', 'Firestorm'),
    (19, 280000, 'Maj General', 'High Siege', 'Praetor', 'Conflagra'),
    (20, 350000, 'Lt General', 'Conquest', 'Imperator', 'Apocalypse'),
    (21, 450000, 'General', 'Grand Conq', 'High Imp', 'Ragnarok'),
    (22, 600000, 'Commander', 'Overlord', 'Sovereign', 'Inferno');
CREATE TABLE discord_multipliers (
    id bigserial PRIMARY KEY,
    name text NOT NULL,
    factor numeric(4, 2) NOT NULL CHECK (factor > 0 AND factor <= 10),
    kind text NOT NULL CHECK (kind IN ('event', 'role', 'channel', 'user', 'faction')),
    target text CHECK ((kind = 'event') = (target IS NULL)),
    starts_at timestamptz NOT NULL DEFAULT now(),
    ends_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
-- Who is in voice now, rebuilt from the server's voice states whenever the bot connects.
CREATE TABLE discord_voice (
    discord_user text PRIMARY KEY,
    channel_id text NOT NULL,
    roles text[] NOT NULL DEFAULT '{}',
    streaming boolean NOT NULL DEFAULT false,
    -- Since when the member has been muted and deafened at once (no voice XP after 15 minutes).
    quiet_since timestamptz,
    credited_at timestamptz NOT NULL DEFAULT now()
);
ALTER TABLE discord_guilds ADD COLUMN afk_channel text;
-- Faction points earned in Discord (shown in Discord; seasons arrive with step 3).
-- ponytail: one row per award; roll up per day if this ever grows large.
CREATE TABLE discord_points (
    id bigserial PRIMARY KEY,
    discord_user text NOT NULL,
    faction text NOT NULL CHECK (faction IN ('myria', 'aetheron', 'glint')),
    kind text NOT NULL CHECK (kind IN ('chat', 'voice', 'stream_support', 'event', 'quest', 'giveaway', 'holiday')),
    points integer NOT NULL CHECK (points >= 0),
    at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX discord_points_faction ON discord_points(faction, at);
CREATE INDEX discord_points_user ON discord_points(discord_user, at);
CREATE TABLE discord_achievements (
    discord_user text NOT NULL,
    key text NOT NULL,
    unlocked_at timestamptz NOT NULL DEFAULT now(),
    announced_at timestamptz,
    PRIMARY KEY (discord_user, key)
);
CREATE INDEX discord_achievements_unannounced ON discord_achievements(unlocked_at) WHERE announced_at IS NULL;
CREATE TABLE discord_quests (
    id bigserial PRIMARY KEY,
    name text NOT NULL,
    description text NOT NULL DEFAULT '',
    period text NOT NULL CHECK (period IN ('daily', 'weekly', 'monthly', 'once')),
    -- What it takes, by kind: {"chat_messages": 100}; several kinds must all be met.
    needs jsonb NOT NULL CHECK (jsonb_typeof(needs) = 'object' AND needs <> '{}'),
    xp integer NOT NULL DEFAULT 0 CHECK (xp >= 0),
    points integer NOT NULL DEFAULT 0 CHECK (points >= 0),
    role text,
    faction text CHECK (faction IN ('myria', 'aetheron', 'glint')),
    max_completions integer CHECK (max_completions > 0),
    ends_at timestamptz,
    active boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE discord_quest_progress (
    quest_id bigint NOT NULL REFERENCES discord_quests(id) ON DELETE CASCADE,
    discord_user text NOT NULL,
    -- 2026-10-19, week-2026-10-19 (its Monday), 2026-10 or once.
    period_key text NOT NULL,
    progress jsonb NOT NULL DEFAULT '{}',
    completed_at timestamptz,
    PRIMARY KEY (quest_id, discord_user, period_key)
);
CREATE INDEX discord_quest_completions ON discord_quest_progress(discord_user) WHERE completed_at IS NOT NULL;
-- Completion milestones (5, 10, 25, 50, 100) already paid.
CREATE TABLE discord_quest_milestones (
    discord_user text NOT NULL,
    completions integer NOT NULL,
    PRIMARY KEY (discord_user, completions)
);
