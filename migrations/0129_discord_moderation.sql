-- Moderation and safety on the S.V.E.R home server (docs/DISCORD_BOT.md "Moderation and safety").
-- Thresholds and word lists are settings in discord_home, not here.

-- Recent messages: automod's rate checks read them, and they are the evidence when a message is
-- deleted or edited. Kept a day, or 30 days once deleted.
CREATE TABLE discord_messages (
    id text PRIMARY KEY,
    channel_id text NOT NULL,
    author_id text NOT NULL,
    content text NOT NULL DEFAULT '',
    attachments jsonb NOT NULL DEFAULT '[]',
    created_at timestamptz NOT NULL DEFAULT now(),
    edited_at timestamptz,
    deleted_at timestamptz
);
CREATE INDEX discord_messages_author ON discord_messages (author_id, created_at);
CREATE INDEX discord_messages_created ON discord_messages (created_at);

-- Every strike, action and note, one row each. A strike is an active 'warn'; a timed ban or mute
-- has expires_at, and lifted_at once it's over. moderator_id NULL means automod.
CREATE TABLE discord_cases (
    id bigserial PRIMARY KEY,
    user_id text NOT NULL,
    moderator_id text,
    action text NOT NULL CHECK (action IN ('warn', 'note', 'mute', 'unmute', 'kick', 'ban', 'softban', 'unban')),
    reason text NOT NULL DEFAULT '',
    active boolean NOT NULL DEFAULT true,
    expires_at timestamptz,
    lifted_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX discord_cases_user ON discord_cases (user_id, created_at);
CREATE INDEX discord_cases_expiring ON discord_cases (expires_at) WHERE lifted_at IS NULL;

-- A proposed escalation, waiting for staff in the mod log; one open at a time per member.
CREATE TABLE discord_escalations (
    id bigserial PRIMARY KEY,
    user_id text NOT NULL,
    strikes int NOT NULL,
    action text NOT NULL CHECK (action IN ('mute', 'kick', 'ban')),
    minutes int,
    message_id text,
    created_at timestamptz NOT NULL DEFAULT now(),
    repinged_at timestamptz,
    resolved_at timestamptz,
    resolved_by text,
    outcome text
);
CREATE UNIQUE INDEX discord_escalations_open ON discord_escalations (user_id) WHERE resolved_at IS NULL;

-- Joins of the last day, for raid detection and "ban all".
CREATE TABLE discord_joins (
    user_id text NOT NULL,
    joined_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX discord_joins_at ON discord_joins (joined_at);

-- A lockdown (manual or anti-raid): each channel's overwrite for the role before the lock, so
-- unlocking restores it exactly.
CREATE TABLE discord_lockdowns (
    id bigserial PRIMARY KEY,
    raid_joins int,
    raiders text[] NOT NULL DEFAULT '{}',
    channel_id text,
    role_id text NOT NULL,
    previous jsonb NOT NULL DEFAULT '{}',
    locked_by text,
    created_at timestamptz NOT NULL DEFAULT now(),
    unlock_at timestamptz,
    unlocked_at timestamptz,
    unlocked_by text
);
CREATE INDEX discord_lockdowns_active ON discord_lockdowns (created_at) WHERE unlocked_at IS NULL;

-- Safety-layer signals (nudges, mentions, caps grace, alerts already sent), kept 30 days.
CREATE TABLE discord_signals (
    kind text NOT NULL,
    user_id text NOT NULL,
    target_id text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX discord_signals_target ON discord_signals (kind, target_id, created_at);
CREATE INDEX discord_signals_user ON discord_signals (kind, user_id, created_at);

-- Questions in support channels, until someone replies.
CREATE TABLE discord_questions (
    message_id text PRIMARY KEY,
    channel_id text NOT NULL,
    user_id text NOT NULL,
    asked_at timestamptz NOT NULL DEFAULT now(),
    answered_at timestamptz,
    alerted_at timestamptz
);

-- Tickets and modmail: a private thread per ticket, one open ticket per member.
CREATE TABLE discord_tickets (
    id bigserial PRIMARY KEY,
    user_id text NOT NULL,
    thread_id text UNIQUE,
    category text NOT NULL DEFAULT 'support' CHECK (category IN ('support', 'appeal', 'report', 'other')),
    priority text NOT NULL DEFAULT 'normal' CHECK (priority IN ('low', 'normal', 'high', 'urgent')),
    status text NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'claimed', 'closed')),
    claimed_by text,
    created_at timestamptz NOT NULL DEFAULT now(),
    active_at timestamptz NOT NULL DEFAULT now(),
    warned_at timestamptz,
    closed_at timestamptz,
    closed_by text,
    close_reason text,
    dm_blocked boolean NOT NULL DEFAULT false
);
CREATE UNIQUE INDEX discord_tickets_open ON discord_tickets (user_id) WHERE status <> 'closed';
CREATE TABLE discord_ticket_messages (
    id bigserial PRIMARY KEY,
    ticket_id bigint NOT NULL REFERENCES discord_tickets(id) ON DELETE CASCADE,
    author_id text NOT NULL,
    kind text NOT NULL CHECK (kind IN ('user', 'staff', 'note')),
    content text NOT NULL DEFAULT '',
    attachments jsonb NOT NULL DEFAULT '[]',
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX discord_ticket_messages_ticket ON discord_ticket_messages (ticket_id, id);
