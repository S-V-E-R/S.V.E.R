-- Outside mutes keep the sender's name (docs/LINKED_CHAT.md): their messages expire after 7 days,
-- and a permanent mute must still say who it is. Filled from their newest message while it exists.
-- The index finds a sender's newest message here, for this fill and for each new mute.
CREATE INDEX outside_chat_sender ON outside_chat_messages (channel_id, platform, sender_id, seq DESC);
ALTER TABLE outside_chat_mutes ADD COLUMN sender_name text;
UPDATE outside_chat_mutes m SET sender_name=(SELECT o.sender_name FROM outside_chat_messages o
    WHERE o.channel_id=m.channel_id AND o.platform=m.platform AND o.sender_id=m.sender_id ORDER BY o.seq DESC LIMIT 1);
