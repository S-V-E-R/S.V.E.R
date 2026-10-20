-- Outside messages and the bot's lines expire after 7 days like S.V.E.R chat (docs/LINKED_CHAT.md);
-- the jobs pass deletes them oldest first.
CREATE INDEX outside_chat_expiry ON outside_chat_messages (expires_at);
