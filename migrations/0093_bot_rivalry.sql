-- Optional cross-faction raid greetings (docs/COMMUNITY.md, docs/LORE.md "Rivalries"): off by
-- default. A raid remembers the raider's faction so the bot can add its faction's jab.
ALTER TABLE chat_settings ADD COLUMN bot_rivalry boolean NOT NULL DEFAULT false;
ALTER TABLE bot_events ADD COLUMN faction text;
