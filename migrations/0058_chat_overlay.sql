-- Pop-out chat (docs/CHANNEL_ADDITIONS.md): how long messages stay on the OBS chat overlay.
ALTER TABLE chat_settings ADD COLUMN overlay_fade_seconds smallint NOT NULL DEFAULT 30 CHECK (overlay_fade_seconds BETWEEN 10 AND 120);
