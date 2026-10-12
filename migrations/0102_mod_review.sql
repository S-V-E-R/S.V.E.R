-- Act first, review later (Joe, October 12, 2026): a timeout or ban given without a reason takes
-- effect at once and waits here until its moderator (or the owner) keeps it with a reason or undoes
-- it. The moderator is notified when the stream ends; unreviewed bans stay and are flagged to the owner.
ALTER TABLE channel_moderation_log ADD COLUMN needs_review boolean NOT NULL DEFAULT false;
CREATE INDEX channel_moderation_log_review ON channel_moderation_log(channel_id, actor_id) WHERE needs_review;
ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check CHECK(kind IN ('live','guild_application','guild_decision','guild_invite','squad_invite','sub_ending','hook_disabled','dm','light','mod_review'));
