-- Turns at the top of Live Now that someone saw (docs/MAGNET.md "What the streamer sees"): one
-- row per 3-minute rotation slot in which the homepage was loaded, naming the stream on top.
-- Kept 30 days; shown only to that streamer, never used for scoring.
CREATE TABLE rotation_turns (
    slot bigint PRIMARY KEY,
    broadcast_id text NOT NULL REFERENCES broadcasts(id) ON DELETE CASCADE
);
CREATE INDEX rotation_turns_broadcast ON rotation_turns(broadcast_id);
