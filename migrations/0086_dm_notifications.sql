-- Direct message notifications (docs/COMMUNITY.md "Direct messages").
ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check CHECK(kind IN ('live','guild_application','guild_decision','guild_invite','squad_invite','sub_ending','hook_disabled','dm'));
