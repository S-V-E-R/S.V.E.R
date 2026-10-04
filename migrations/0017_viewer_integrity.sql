-- Module 3 viewer integrity (docs/LIVE_STREAMS.md, "Viewer integrity"). Each playback lease gets a
-- level: pending, counted (public count), trusted (public count + tiers, payouts, influence) or
-- excluded. Raw IP addresses are never stored: only keyed hashes, re-keyed every 30 days.
ALTER TABLE playback_leases
    ADD COLUMN level text NOT NULL DEFAULT 'pending'
        CHECK (level IN ('pending', 'counted', 'trusted', 'excluded')),
    ADD COLUMN risk real NOT NULL DEFAULT 0,
    ADD COLUMN flags text[] NOT NULL DEFAULT '{}',
    -- Hard evidence keeps a session excluded for the rest of the broadcast.
    ADD COLUMN hard_excluded boolean NOT NULL DEFAULT false,
    ADD COLUMN signed_in boolean NOT NULL DEFAULT false,
    -- Signed in, email verified and in good standing at the last heartbeat.
    ADD COLUMN verified boolean NOT NULL DEFAULT false,
    ADD COLUMN turnstile_ok boolean NOT NULL DEFAULT false,
    ADD COLUMN ip_hash text,
    ADD COLUMN net_hash text,
    ADD COLUMN beats integer NOT NULL DEFAULT 0,
    ADD COLUMN last_beat_at timestamptz,
    -- Running mean and sum of squared deviations (Welford) of heartbeat intervals, in ms.
    ADD COLUMN interval_count integer NOT NULL DEFAULT 0,
    ADD COLUMN interval_mean double precision NOT NULL DEFAULT 0,
    ADD COLUMN interval_m2 double precision NOT NULL DEFAULT 0,
    ADD COLUMN visible_seconds double precision NOT NULL DEFAULT 0,
    ADD COLUMN last_media_time double precision,
    ADD COLUMN ahead_strikes integer NOT NULL DEFAULT 0,
    ADD COLUMN provisional_until timestamptz,
    ADD COLUMN ever_counted boolean NOT NULL DEFAULT false;
CREATE INDEX playback_leases_network ON playback_leases (broadcast_id, net_hash) WHERE net_hash IS NOT NULL;
CREATE INDEX playback_leases_arrivals ON playback_leases (broadcast_id, created_at);

-- Per live broadcast, about once a minute. Kept one year.
CREATE TABLE integrity_snapshots (
    broadcast_id text NOT NULL REFERENCES broadcasts(id) ON DELETE CASCADE,
    taken_at timestamptz NOT NULL DEFAULT now(),
    raw integer NOT NULL,
    counted integer NOT NULL,
    trusted integer NOT NULL,
    excluded integer NOT NULL,
    pending integer NOT NULL,
    PRIMARY KEY (broadcast_id, taken_at)
);

-- Opened automatically when a broadcast's excluded share stays high; only staff decide. The
-- evidence is aggregate (counts, flag totals, network counts), never addresses or hashes.
CREATE TABLE integrity_cases (
    id text PRIMARY KEY,
    broadcast_id text NOT NULL,
    owner_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    opened_at timestamptz NOT NULL DEFAULT now(),
    evidence jsonb NOT NULL,
    status text NOT NULL DEFAULT 'OPEN' CHECK (status IN ('OPEN', 'DISMISSED', 'ACTIONED')),
    -- Recorded for Module 6 (payouts, tiers) to honor; strikes go through the standing tools.
    hold_payouts boolean NOT NULL DEFAULT false,
    pause_tier boolean NOT NULL DEFAULT false,
    decided_by text,
    decided_at timestamptz,
    note text NOT NULL DEFAULT '',
    CHECK ((status = 'OPEN') = (decided_at IS NULL))
);
CREATE UNIQUE INDEX integrity_cases_one_open ON integrity_cases (broadcast_id) WHERE status = 'OPEN';
CREATE INDEX integrity_cases_owner ON integrity_cases (owner_id, opened_at DESC);
