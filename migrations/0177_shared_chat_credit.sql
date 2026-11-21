-- Merged co-stream shared chat (docs/SUPPORT.md "Co-streams"): the member channel a shared line
-- credited (Engagement, faction influence, Surge), so its loyalty badge is that channel's; and the
-- channel bot's command replies in a shared chat, kept with that chat, not the channel's own.
ALTER TABLE chat_messages ADD COLUMN credit_id text;
ALTER TABLE outside_chat_messages ADD COLUMN squad_id text REFERENCES squads(id) ON DELETE CASCADE;
CREATE INDEX outside_chat_shared ON outside_chat_messages (squad_id, seq DESC) WHERE squad_id IS NOT NULL;
