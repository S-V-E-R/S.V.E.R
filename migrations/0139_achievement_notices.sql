-- Achievement unlock and creator tier promotion notices (docs/ACHIEVEMENTS.md).
ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check CHECK(kind IN ('live','guild_application','guild_decision','guild_invite','squad_invite','sub_ending','hook_disabled','dm','light','mod_review','good_works','achievement','tier'));
-- Rarity counts holders per achievement.
CREATE INDEX user_achievements_key ON user_achievements(key);
