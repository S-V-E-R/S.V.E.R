-- Third-party emotes (docs/DEVELOPER_PLATFORM.md §7).
-- An image that fails to copy (missing, too big, not an image) is retried after a backoff of
-- least(24, 2^attempts) hours instead of every minute, so it can't hold up other emotes.
CREATE TABLE outside_emote_failures (
    provider text NOT NULL,
    emote_id text NOT NULL,
    failed_at timestamptz NOT NULL DEFAULT now(),
    attempts int NOT NULL DEFAULT 1,
    PRIMARY KEY (provider, emote_id)
);

-- Animated emotes also get a first-frame still at each size, shown to viewers who reduce motion.
ALTER TABLE outside_emote_files ADD COLUMN still boolean NOT NULL DEFAULT false;

-- The streamer's Show is remembered ('shown'), so a later viewer report can't hide it again.
ALTER TABLE outside_emote_hides
    DROP CONSTRAINT outside_emote_hides_reason_check,
    ADD CONSTRAINT outside_emote_hides_reason_check CHECK (reason IN ('streamer','report','shown'));
