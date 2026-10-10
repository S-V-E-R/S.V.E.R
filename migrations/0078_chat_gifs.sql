-- GIFs in chat (docs/COMMUNITY.md "GIFs in chat"): KLIPY media referenced by URL, never stored.
ALTER TABLE chat_messages ADD COLUMN gif jsonb;
ALTER TABLE chat_settings ADD COLUMN gifs text NOT NULL DEFAULT 'everyone'
    CHECK (gifs IN ('off', 'everyone', 'followers', 'subscribers'));
