-- Module 3 viewer counting: one lease per viewer per broadcast, renewed by heartbeats
-- from a player whose media is advancing. A viewer counts while expires_at is in the future.
CREATE TABLE playback_leases (
    broadcast_id text NOT NULL REFERENCES broadcasts(id) ON DELETE CASCADE,
    -- 'u:<user id>' for signed-in viewers, 'b:<digest of a random browser ID>' for guests.
    viewer_key text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    PRIMARY KEY (broadcast_id, viewer_key)
);
CREATE INDEX playback_leases_expiry ON playback_leases(expires_at);
