-- Module 6 Support, the rest (docs/SUPPORT.md): creator tiers, paydays and Early Pay, pooled money
-- in merged co-streams, and Shine (charity streams and Good Works badges).

-- Creator tiers: 0 Scout, 1 Trailblazer, 2 Pioneer, 3 Pathfinder. Tiers never go down.
CREATE TABLE creator_tiers (
    user_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    tier smallint NOT NULL DEFAULT 0 CHECK (tier BETWEEN 0 AND 3),
    promoted_at timestamptz,
    checked_at timestamptz
);
-- Unique trusted viewers per channel (leases expire; this keeps who watched for the 90-day window).
CREATE TABLE creator_viewers (
    owner_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    viewer_key text NOT NULL,
    last_seen timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (owner_id, viewer_key)
);
CREATE INDEX creator_viewers_recent ON creator_viewers (owner_id, last_seen);
-- Scheduled work that must run once per period (weekly tier checks, paydays).
CREATE TABLE support_runs (
    kind text NOT NULL CHECK (kind IN ('tiers', 'payday')),
    period timestamptz NOT NULL,
    ran_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (kind, period)
);

-- Every payout: a Stripe transfer to the creator's Express account (and, for instant Early Pay, an
-- instant payout from it). Pending runs count toward Early Pay limits until they settle.
CREATE TABLE payout_runs (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('payday', 'early_standard', 'early_instant')),
    amount_cents bigint NOT NULL CHECK (amount_cents > 0),
    fee_cents bigint NOT NULL DEFAULT 0 CHECK (fee_cents >= 0),
    status text NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'paid', 'failed')),
    transfer_id text,
    payout_id text,
    error text,
    created_at timestamptz NOT NULL DEFAULT now(),
    completed_at timestamptz
);
CREATE INDEX payout_runs_user ON payout_runs (user_id, created_at DESC);

-- A merged co-stream's first-month subscription share is split among the members live then, each
-- at their own tier split: [[member, share in tenths of a cent], ...] (NULL: all to channel_id).
ALTER TABLE sub_invoices ADD COLUMN shares jsonb;

-- Shine: the next stream's charity (set in Creator Studio), copied to the broadcast at go-live.
ALTER TABLE stream_settings
    ADD COLUMN charity_name text CHECK (charity_name IS NULL OR char_length(charity_name) BETWEEN 1 AND 80),
    ADD COLUMN charity_url text CHECK (charity_url IS NULL OR char_length(charity_url) BETWEEN 8 AND 500);
-- One per charity broadcast. After it ends the streamer can submit the amount raised with proof;
-- staff verify it into a Good Works badge (or reject it, or later revoke it).
CREATE TABLE charity_streams (
    id text PRIMARY KEY,
    owner_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    broadcast_id text UNIQUE REFERENCES broadcasts(id) ON DELETE SET NULL,
    charity_name text NOT NULL,
    donate_url text NOT NULL,
    started_at timestamptz NOT NULL DEFAULT now(),
    raised_cents bigint CHECK (raised_cents IS NULL OR raised_cents > 0),
    proof_url text CHECK (proof_url IS NULL OR char_length(proof_url) BETWEEN 8 AND 500),
    status text NOT NULL DEFAULT 'none' CHECK (status IN ('none', 'submitted', 'verified', 'rejected', 'revoked')),
    submitted_at timestamptz,
    reviewed_by text REFERENCES users(id) ON DELETE SET NULL,
    reviewed_at timestamptz,
    review_note text
);
CREATE INDEX charity_streams_owner ON charity_streams (owner_id, started_at DESC);
CREATE INDEX charity_streams_review ON charity_streams (submitted_at) WHERE status = 'submitted';
