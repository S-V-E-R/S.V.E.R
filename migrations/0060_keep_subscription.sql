-- Keep a gifted subscription (docs/CHANNEL_ADDITIONS.md): an in-site reminder three days before a
-- gifted (or Valor) month ends.
ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check CHECK(kind IN ('live','guild_application','guild_decision','guild_invite','squad_invite','sub_ending'));
