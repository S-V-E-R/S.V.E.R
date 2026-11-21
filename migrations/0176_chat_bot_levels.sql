-- AutoMod strictness per rule (0 off, 1 relaxed, 2 standard, 3 strict). A rule that was on becomes
-- standard, the thresholds it used before. Bot announcements: the events a streamer turned off, and
-- their own line per event (docs/COMMUNITY.md "Chat commands and the channel bot").
ALTER TABLE chat_settings
    ALTER COLUMN automod_caps DROP DEFAULT,
    ALTER COLUMN automod_caps TYPE smallint USING CASE WHEN automod_caps THEN 2 ELSE 0 END,
    ALTER COLUMN automod_caps SET DEFAULT 0,
    ALTER COLUMN automod_repeats DROP DEFAULT,
    ALTER COLUMN automod_repeats TYPE smallint USING CASE WHEN automod_repeats THEN 2 ELSE 0 END,
    ALTER COLUMN automod_repeats SET DEFAULT 0,
    ALTER COLUMN automod_spam DROP DEFAULT,
    ALTER COLUMN automod_spam TYPE smallint USING CASE WHEN automod_spam THEN 2 ELSE 0 END,
    ALTER COLUMN automod_spam SET DEFAULT 0,
    ADD CONSTRAINT automod_levels CHECK (automod_caps BETWEEN 0 AND 3 AND automod_repeats BETWEEN 0 AND 3 AND automod_spam BETWEEN 0 AND 3),
    ADD COLUMN bot_off text[] NOT NULL DEFAULT '{}',
    ADD COLUMN bot_lines jsonb NOT NULL DEFAULT '{}';
