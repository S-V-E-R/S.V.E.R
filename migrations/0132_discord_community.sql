-- Community features on the home server (docs/DISCORD_BOT.md "Community"): onboarding, birthdays,
-- anniversaries, seasonal events, events and RSVPs, polls, giveaways, reminders, mentors, managed
-- threads, temp voice rooms, custom commands, free games and WoW characters. Discord users are
-- keyed by their Discord id. Everything that must survive a restart is a row.

-- DM onboarding: the step a member is on, and when they finished.
ALTER TABLE discord_members ADD COLUMN onboarding text
        CHECK (onboarding IN ('rules', 'faction', 'platforms', 'link', 'birthday')),
    ADD COLUMN onboarded_at timestamptz;

-- A birthday in the member's own IANA zone; celebrated once a year (changing it never repeats it).
CREATE TABLE discord_birthdays (
    discord_user text PRIMARY KEY,
    month smallint NOT NULL CHECK (month BETWEEN 1 AND 12),
    day smallint NOT NULL CHECK (day BETWEEN 1 AND 31),
    year smallint CHECK (year BETWEEN 1900 AND 2100),
    zone text NOT NULL,
    celebrated_year smallint,
    -- The birthday role comes off at the end of their day.
    role_until timestamptz
);
CREATE INDEX discord_birthdays_role ON discord_birthdays(role_until) WHERE role_until IS NOT NULL;

-- Server anniversaries already celebrated.
CREATE TABLE discord_anniversaries (
    discord_user text NOT NULL,
    years smallint NOT NULL,
    at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (discord_user, years)
);

-- Seasonal events (/rituals event): announced once at the start and once at the end.
CREATE TABLE discord_seasonal_events (
    id bigserial PRIMARY KEY,
    name text NOT NULL,
    description text NOT NULL DEFAULT '',
    theme text NOT NULL DEFAULT 'custom' CHECK (theme IN ('winter', 'spring', 'summer', 'fall', 'custom')),
    starts_at timestamptz NOT NULL,
    ends_at timestamptz NOT NULL,
    factor numeric(4, 2) CHECK (factor > 0 AND factor <= 10),
    multiplier_id bigint REFERENCES discord_multipliers(id) ON DELETE SET NULL,
    started_at timestamptz,
    ended_at timestamptz,
    created_by text,
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK (starts_at < ends_at)
);

-- Events with RSVPs (/event), in their own time zone, optionally repeating.
CREATE TABLE discord_events (
    id bigserial PRIMARY KEY,
    title text NOT NULL,
    description text,
    kind text NOT NULL DEFAULT 'general'
        CHECK (kind IN ('general', 'game_night', 'watch_party', 'faction_war', 'territory_push', 'tournament', 'custom')),
    faction text CHECK (faction IN ('myria', 'aetheron', 'glint')),
    zone text NOT NULL,
    starts_at timestamptz NOT NULL,
    ends_at timestamptz,
    repeat text CHECK (repeat IN ('daily', 'weekly', 'biweekly', 'monthly')),
    status text NOT NULL DEFAULT 'scheduled' CHECK (status IN ('scheduled', 'active', 'completed', 'cancelled')),
    channel_id text,
    message_id text,
    reminded_60 boolean NOT NULL DEFAULT false,
    reminded_15 boolean NOT NULL DEFAULT false,
    -- Staff events ping the faction's role in their reminder.
    ping_role boolean NOT NULL DEFAULT false,
    created_by text NOT NULL,
    parent_id bigint REFERENCES discord_events(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX discord_events_due ON discord_events(starts_at) WHERE status IN ('scheduled', 'active');
CREATE TABLE discord_event_rsvps (
    event_id bigint NOT NULL REFERENCES discord_events(id) ON DELETE CASCADE,
    discord_user text NOT NULL,
    status text NOT NULL CHECK (status IN ('going', 'maybe', 'not_going')),
    -- The first "going" earns the event's points and quest progress; switching back never does.
    counted boolean NOT NULL DEFAULT false,
    at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (event_id, discord_user)
);

-- Polls (/poll): one vote each, closed at their time.
CREATE TABLE discord_polls (
    id bigserial PRIMARY KEY,
    question text NOT NULL,
    options text[] NOT NULL CHECK (cardinality(options) BETWEEN 2 AND 4),
    channel_id text NOT NULL,
    message_id text,
    created_by text NOT NULL,
    ends_at timestamptz,
    closed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX discord_polls_open ON discord_polls(ends_at) WHERE closed_at IS NULL;
CREATE TABLE discord_poll_votes (
    poll_id bigint NOT NULL REFERENCES discord_polls(id) ON DELETE CASCADE,
    discord_user text NOT NULL,
    option smallint NOT NULL,
    PRIMARY KEY (poll_id, discord_user)
);

-- Giveaways: submitted for review or made by staff, entered by button or 🎉, drawn on time.
CREATE TABLE discord_giveaways (
    id bigserial PRIMARY KEY,
    prize text NOT NULL,
    description text,
    winners smallint NOT NULL DEFAULT 1 CHECK (winners BETWEEN 1 AND 20),
    minutes integer NOT NULL CHECK (minutes > 0),
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'active', 'ended', 'cancelled')),
    channel_id text,
    message_id text,
    created_by text NOT NULL,
    reviewed_by text,
    deny_reason text,
    starts_at timestamptz,
    ends_at timestamptz,
    ended_at timestamptz,
    winner_ids text[] NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX discord_giveaways_due ON discord_giveaways(ends_at) WHERE status = 'active';
CREATE INDEX discord_giveaways_message ON discord_giveaways(message_id);
CREATE TABLE discord_giveaway_entries (
    giveaway_id bigint NOT NULL REFERENCES discord_giveaways(id) ON DELETE CASCADE,
    discord_user text NOT NULL,
    at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (giveaway_id, discord_user)
);

CREATE TABLE discord_reminders (
    id bigserial PRIMARY KEY,
    discord_user text NOT NULL,
    channel_id text NOT NULL,
    message text NOT NULL,
    remind_at timestamptz NOT NULL,
    fired_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX discord_reminders_due ON discord_reminders(remind_at) WHERE fired_at IS NULL;

CREATE TABLE discord_mentors (
    discord_user text PRIMARY KEY,
    platforms text[] NOT NULL DEFAULT '{}',
    specialties text[] NOT NULL DEFAULT '{}',
    games text[] NOT NULL DEFAULT '{}',
    availability text,
    max_mentees smallint NOT NULL DEFAULT 3,
    active boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now()
);
-- One suggested or running mentorship per row; a mentor who passes leaves a 'passed' row.
CREATE TABLE discord_mentorships (
    id bigserial PRIMARY KEY,
    mentor text NOT NULL,
    mentee text NOT NULL,
    help_with text NOT NULL,
    platform text,
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'active', 'completed', 'passed', 'cancelled')),
    thread_id text,
    created_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz
);
CREATE UNIQUE INDEX discord_mentorships_one_open ON discord_mentorships(mentee) WHERE status IN ('pending', 'active');

-- Threads and forum posts the bot made (managed channels, starter posts, WoW threads).
CREATE TABLE discord_threads (
    thread_id text PRIMARY KEY,
    channel_id text NOT NULL,
    name text NOT NULL,
    category text,
    created_by text,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX discord_threads_category ON discord_threads(category, created_at DESC);

-- Temporary voice rooms, deleted once empty.
CREATE TABLE discord_temp_rooms (
    channel_id text PRIMARY KEY,
    owner text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE discord_custom_commands (
    name text PRIMARY KEY CHECK (name ~ '^[a-z0-9_-]{1,32}$'),
    response text NOT NULL CHECK (length(response) BETWEEN 1 AND 2000),
    created_by text NOT NULL,
    uses integer NOT NULL DEFAULT 0,
    created_at timestamptz NOT NULL DEFAULT now()
);

-- Free games already posted, so each is posted once.
CREATE TABLE discord_free_games (
    store text NOT NULL CHECK (store IN ('epic', 'steam', 'gog')),
    title text NOT NULL,
    url text NOT NULL,
    expires_at timestamptz,
    message_id text,
    posted_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (store, title)
);

CREATE TABLE discord_wow (
    discord_user text PRIMARY KEY,
    character text NOT NULL,
    realm text NOT NULL,
    linked_at timestamptz NOT NULL DEFAULT now()
);
-- Every /wow submit and what Chromely said (Chromely holds the runs themselves).
CREATE TABLE discord_wow_runs (
    id bigserial PRIMARY KEY,
    discord_user text NOT NULL,
    character text NOT NULL,
    realm text NOT NULL,
    seconds double precision NOT NULL,
    proof_url text,
    status text NOT NULL CHECK (status IN ('accepted', 'rejected', 'error')),
    reason text,
    submitted_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX discord_wow_runs_user ON discord_wow_runs(discord_user, submitted_at DESC);
