-- YouTube linked chat (docs/LINKED_CHAT.md): YouTube has no chat webhooks, so the supervisor polls
-- the streamer's active broadcast's live chat. Where it is and how far it has read survive a
-- restart: the live chat's ID, YouTube's page token, and when to read next.
ALTER TABLE linked_chat_accounts ADD COLUMN chat_id text, ADD COLUMN page_token text, ADD COLUMN poll_at timestamptz;
