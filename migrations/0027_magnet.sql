-- Module 5 MAGNet Hype (docs/MAGNET.md "MAGNet Hype"): one Global lane and one lane per genre,
-- each its own engine. Viewer count, follower totals and money are not stored here or used.
CREATE TABLE magnet_lanes (
    id text PRIMARY KEY,                -- 'global' or a faction_genres id
    enabled boolean NOT NULL DEFAULT true,
    current_broadcast text REFERENCES broadcasts(id) ON DELETE SET NULL,
    current_kind text CHECK (current_kind IN ('moment', 'fair', 'only', 'forced', 'fallback')),
    current_reason text,
    current_since timestamptz,
    -- A switch is announced 5 seconds ahead with a still preview of the next stream.
    pending_broadcast text REFERENCES broadcasts(id) ON DELETE SET NULL,
    pending_kind text CHECK (pending_kind IN ('moment', 'fair', 'only', 'forced', 'fallback')),
    pending_reason text,
    switch_at timestamptz,
    -- The kind of the last moment/fair switch, so the two alternate.
    last_kind text CHECK (last_kind IN ('moment', 'fair')),
    last_moment_at timestamptz,
    forced_broadcast text REFERENCES broadcasts(id) ON DELETE SET NULL,
    ticked_at timestamptz
);
-- Every feature: fair-turn waits, the 30-minute cooldown, first-feature labels and Studio history.
CREATE TABLE magnet_features (
    id text PRIMARY KEY,
    lane text NOT NULL REFERENCES magnet_lanes(id) ON DELETE CASCADE,
    broadcast_id text NOT NULL REFERENCES broadcasts(id) ON DELETE CASCADE,
    owner_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind text NOT NULL,
    reason text NOT NULL,
    started_at timestamptz NOT NULL DEFAULT now(),
    ended_at timestamptz
);
CREATE INDEX magnet_features_owner ON magnet_features (owner_id, started_at DESC);
CREATE INDEX magnet_features_lane ON magnet_features (lane, started_at DESC);
-- Each decision with its candidates, signals and reason; kept 7 days.
CREATE TABLE magnet_decisions (
    id bigserial PRIMARY KEY,
    lane text NOT NULL REFERENCES magnet_lanes(id) ON DELETE CASCADE,
    at timestamptz NOT NULL DEFAULT now(),
    kind text NOT NULL,
    chosen text,
    reason text NOT NULL,
    candidates jsonb NOT NULL DEFAULT '[]'
);
CREATE INDEX magnet_decisions_recent ON magnet_decisions (lane, at DESC);
-- No row means the defaults: featured in MAGNet Hype, chat merging on.
CREATE TABLE magnet_settings (
    user_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    opt_out boolean NOT NULL DEFAULT false,
    chat_merge boolean NOT NULL DEFAULT true
);
-- "Flag this moment": counts only together with another elevated signal; once every 10 minutes.
CREATE TABLE magnet_flags (
    id bigserial PRIMARY KEY,
    owner_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    broadcast_id text NOT NULL REFERENCES broadcasts(id) ON DELETE CASCADE,
    at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX magnet_flags_recent ON magnet_flags (broadcast_id, at DESC);
-- Playback sessions started from a Hype lane, for the streamer's feature history only.
ALTER TABLE playback_leases ADD COLUMN magnet_lane text;
-- Hype chat (docs/MAGNET.md "Hype chat"): NULL is a channel's own chat; otherwise the Hype lane a
-- message came from. Hype-side messages never count toward a stream's chat-burst signal.
ALTER TABLE chat_messages ADD COLUMN origin text;
-- A message in a Hype lane's own room belongs to no channel; every message belongs to one or the other.
ALTER TABLE chat_messages ALTER COLUMN channel_id DROP NOT NULL;
ALTER TABLE chat_messages ADD CONSTRAINT chat_messages_room CHECK (channel_id IS NOT NULL OR origin IS NOT NULL);
CREATE INDEX chat_messages_hype_room ON chat_messages (origin, seq DESC) WHERE channel_id IS NULL AND deleted_at IS NULL;
