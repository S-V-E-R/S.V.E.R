-- Phase 4 ads, design first (docs/ADS.md, Joe's decision October 10, 2026). Ad segments and who
-- watched them, settled into a separate test book of the ledger (accounts prefixed 'adtest:'), so
-- nothing here can reach payouts. No ad provider exists yet; 'test' is the only book allowed.
CREATE TABLE ad_segments (
    id text PRIMARY KEY,
    book text NOT NULL DEFAULT 'test' CHECK (book = 'test'),
    broadcast_id text NOT NULL REFERENCES broadcasts(id) ON DELETE CASCADE,
    owner_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_by text REFERENCES users(id) ON DELETE SET NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    -- Revenue reported for the segment, in tenths of a cent, and the split applied at settlement.
    revenue bigint CHECK (revenue IS NULL OR revenue > 0),
    split jsonb,
    settled_at timestamptz
);
CREATE INDEX ad_segments_recent ON ad_segments (created_at DESC);
-- Viewers counted for a segment: trusted, signed in, not the owner and not subscribed (subscribers
-- see no ads on that channel), snapshotted when the break starts because leases expire.
CREATE TABLE ad_segment_viewers (
    segment_id text NOT NULL REFERENCES ad_segments(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (segment_id, user_id)
);
