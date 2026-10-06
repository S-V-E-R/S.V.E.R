-- Module 7 CrowdSync, phase 1 (docs/CROWDSYNC.md "Boards"): one board per channel with a draft and
-- a published version, presses that spend Engagement Valor, goals, board blocks, the OBS overlay
-- token and webhook output through a Postgres outbox.

-- The board definition is JSON (screens of controls), validated by the API. Editing changes only
-- the draft; publishing copies it and bumps the version.
CREATE TABLE boards (
    channel_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    draft jsonb NOT NULL,
    published jsonb,
    version integer NOT NULL DEFAULT 0,
    published_at timestamptz,
    updated_at timestamptz NOT NULL DEFAULT now(),
    -- Panic switch: no presses, effects or sounds until turned back on.
    disabled boolean NOT NULL DEFAULT false,
    -- Channel moderators may run the board (panic, blocks).
    moderators_run boolean NOT NULL DEFAULT false,
    -- OBS overlay: only the token's digest is stored; seen_at says it's connected.
    overlay_token_hash text UNIQUE,
    overlay_seen_at timestamptz,
    -- Webhook output: the owner's HTTPS endpoint and its sealed signing secret.
    webhook_url text CHECK (webhook_url IS NULL OR char_length(webhook_url) BETWEEN 9 AND 500),
    webhook_secret text
);

-- Every press, with its Engagement Valor cost, recorded in the same transaction as the charge.
CREATE TABLE board_presses (
    id text PRIMARY KEY,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    broadcast_id text,
    version integer NOT NULL,
    control_id text NOT NULL,
    cost integer NOT NULL CHECK (cost >= 0),
    input jsonb,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX board_presses_control ON board_presses (channel_id, control_id, created_at);
CREATE INDEX board_presses_viewer ON board_presses (channel_id, user_id, control_id, created_at);

-- Viewers the streamer (or a board moderator) blocked from pressing.
CREATE TABLE board_blocks (
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (channel_id, user_id)
);

-- Shared goal progress for the published version (reset on publish).
CREATE TABLE board_goals (
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    control_id text NOT NULL,
    progress integer NOT NULL DEFAULT 0 CHECK (progress >= 0),
    PRIMARY KEY (channel_id, control_id)
);

-- Deliveries to outside systems (webhooks), written with the press and sent by a worker, so a
-- crash can neither lose a press nor deliver one that was rolled back.
CREATE TABLE outbox (
    id bigserial PRIMARY KEY,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('webhook')),
    payload jsonb NOT NULL,
    attempts integer NOT NULL DEFAULT 0,
    available_at timestamptz NOT NULL DEFAULT now(),
    delivered_at timestamptz,
    error text,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX outbox_due ON outbox (available_at) WHERE delivered_at IS NULL;
