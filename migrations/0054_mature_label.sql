-- Mature label (docs/CHANNEL_ADDITIONS.md): saved on the channel, remembered on each broadcast that
-- was labeled at any point (its VOD and clips inherit it). Staff can lock it on until the broadcast
-- ends.
ALTER TABLE stream_settings ADD COLUMN mature boolean NOT NULL DEFAULT false;
ALTER TABLE broadcasts
    ADD COLUMN mature boolean NOT NULL DEFAULT false,
    ADD COLUMN mature_locked boolean NOT NULL DEFAULT false;
