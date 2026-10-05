-- Module 3 spike protection (docs/LIVE_STREAMS.md "Spikes and enforcement"): followers-only chat
-- that an owner or moderator turns on from a one-click prompt. It always has an end time.
ALTER TABLE chat_settings ADD COLUMN followers_only_until timestamptz;
