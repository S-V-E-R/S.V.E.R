-- Private mode for direct messages (docs/COMMUNITY.md "Private mode"; Joe's decision October 11,
-- 2026). Each browser has its own key pair; only the public key is stored. A private message is a
-- ciphertext envelope the server can't open, plus a commitment that lets a recipient prove what a
-- reported message said ("message franking").
CREATE TABLE dm_devices (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    public_key text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX dm_devices_user ON dm_devices (user_id);
ALTER TABLE dm_conversations ADD COLUMN private boolean NOT NULL DEFAULT false;
ALTER TABLE dm_messages ADD COLUMN envelope jsonb;
ALTER TABLE dm_messages ADD COLUMN commitment text;
ALTER TABLE dm_messages ADD CONSTRAINT dm_messages_private CHECK ((envelope IS NULL) = (commitment IS NULL));
