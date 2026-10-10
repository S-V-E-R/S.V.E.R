-- Signature emotes (docs/CHANNEL_ADDITIONS.md): one open emote per channel that, once staff have
-- reviewed it, works in every chat as username/Code; a channel can turn others' off in its chat.
ALTER TABLE channel_emotes ADD COLUMN signature boolean NOT NULL DEFAULT false CHECK (NOT signature OR tier IS NULL);
CREATE UNIQUE INDEX channel_emotes_signature ON channel_emotes (channel_id) WHERE signature;
ALTER TABLE chat_settings ADD COLUMN allow_signatures boolean NOT NULL DEFAULT true;
