-- Chat context stores references, never a second copy of a quoted message body.
-- reply_to intentionally survives retention/erasure so clients can show a tombstone.
ALTER TABLE chat_messages ADD COLUMN reply_to text;
ALTER TABLE chat_messages ADD COLUMN mention_ids text[] NOT NULL DEFAULT '{}';
ALTER TABLE chat_messages ADD COLUMN role text CHECK (role IN ('owner', 'moderator', 'staff'));
UPDATE chat_messages m SET role = CASE
    WHEN author_id=channel_id THEN 'owner'
    WHEN EXISTS(SELECT 1 FROM channel_moderators cm WHERE cm.channel_id=m.channel_id AND cm.user_id=m.author_id) THEN 'moderator'
    WHEN EXISTS(SELECT 1 FROM staff_roles s JOIN users u ON u.id=s.user_id WHERE s.user_id=m.author_id AND u.mfa_enabled) THEN 'staff'
END;

CREATE TABLE chat_pins (
    channel_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    message_id text NOT NULL UNIQUE REFERENCES chat_messages(id) ON DELETE CASCADE
);

-- Channel moderation, platform moderation and Take It Down all clear pins atomically.
CREATE FUNCTION clear_deleted_chat_pin() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    DELETE FROM chat_pins WHERE message_id=NEW.id;
    RETURN NEW;
END;
$$;
CREATE TRIGGER chat_pin_deleted AFTER UPDATE OF deleted_at ON chat_messages
FOR EACH ROW WHEN (NEW.deleted_at IS NOT NULL) EXECUTE FUNCTION clear_deleted_chat_pin();
