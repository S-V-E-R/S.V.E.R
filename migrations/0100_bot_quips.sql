-- Bot quips (docs/COMMUNITY.md; lines in docs/LORE.md "Quips", a draft for Joe's approval): off by
-- default; while live and only when chat has been active, the bot says one now and then.
ALTER TABLE chat_settings ADD COLUMN bot_quips boolean NOT NULL DEFAULT false;
ALTER TABLE chat_settings ADD COLUMN bot_quip_at timestamptz;
