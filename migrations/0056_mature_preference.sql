-- Mature label: Settings -> Preferences "Don't warn me about mature streams" (adults only; default
-- off). docs/CHANNEL_ADDITIONS.md
ALTER TABLE users ADD COLUMN skip_mature_warning boolean NOT NULL DEFAULT false;
