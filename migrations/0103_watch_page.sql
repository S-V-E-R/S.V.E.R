-- Watch page (docs/LIVE_STREAMS.md "Watch page"): this week's Engagement Valor per viewer per
-- channel for the effort leaders box, and viewers' one-tap playback problem reports.

-- Earned this week (Monday 00:00 UTC). A trigger keeps it, so every writer of `earned` (watch,
-- chat, follow, Surge, orders) counts without changing each one. Spending never lowers it.
ALTER TABLE engagement
    ADD COLUMN week_earned bigint NOT NULL DEFAULT 0 CHECK (week_earned >= 0),
    ADD COLUMN week_of date;
CREATE INDEX engagement_week ON engagement (channel_id, week_of, week_earned DESC);

CREATE FUNCTION engagement_week() RETURNS trigger LANGUAGE plpgsql AS $$
DECLARE
    gained bigint := NEW.earned - CASE WHEN TG_OP = 'UPDATE' THEN OLD.earned ELSE 0 END;
    this_week date := date_trunc('week', now() AT TIME ZONE 'UTC')::date;
BEGIN
    IF gained > 0 THEN
        NEW.week_earned := CASE WHEN NEW.week_of = this_week THEN NEW.week_earned + gained ELSE gained END;
        NEW.week_of := this_week;
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER engagement_week BEFORE INSERT OR UPDATE OF earned ON engagement
    FOR EACH ROW EXECUTE FUNCTION engagement_week();

-- "Report playback problem" from the player's settings menu: fixed choices plus what the player
-- measured, so staff can tell a creator's encoder from our delivery. No free text.
CREATE TABLE playback_problems (
    id text PRIMARY KEY,
    broadcast_id text NOT NULL REFERENCES broadcasts(id) ON DELETE CASCADE,
    user_id text REFERENCES users(id) ON DELETE SET NULL,
    kind text NOT NULL CHECK (kind IN ('stutter', 'no_audio', 'wont_load', 'out_of_sync', 'blurry', 'distorted')),
    transport text CHECK (transport IN ('webrtc', 'hls')),
    delay_ms integer CHECK (delay_ms BETWEEN 0 AND 600000),
    height integer CHECK (height BETWEEN 0 AND 4320),
    dropped_frames integer CHECK (dropped_frames >= 0),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX playback_problems_broadcast ON playback_problems (broadcast_id, kind);
