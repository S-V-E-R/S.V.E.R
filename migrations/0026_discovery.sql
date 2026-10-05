-- Module 5 discovery (docs/MAGNET.md "Discovery", "Spotlights", "Thumbnails").
-- 0025 is reserved by the S.V.E.R Plays work.

-- Staff spotlights: a short public reason, at most 14 days, one active per channel, then a 7-day
-- cooldown. Automatic spotlights (first stream, return after 30+ days) are derived from broadcasts.
CREATE TABLE spotlights (
    id text PRIMARY KEY,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    reason text NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 120),
    starts_at timestamptz NOT NULL DEFAULT now(),
    ends_at timestamptz NOT NULL,
    created_by text REFERENCES users(id) ON DELETE SET NULL,
    ended_early_at timestamptz,
    CHECK (ends_at > starts_at AND ends_at <= starts_at + interval '14 days')
);
CREATE INDEX spotlights_channel ON spotlights (channel_id, ends_at DESC);

-- The latest still frame of a live stream (one decoded keyframe about once a minute).
ALTER TABLE broadcasts ADD COLUMN thumbnail_key text;
ALTER TABLE broadcasts ADD COLUMN thumbnail_at timestamptz;
