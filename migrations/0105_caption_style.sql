-- The channel's default caption size and position (the caption block's center, as fractions of the
-- player). Viewers can resize and drag captions for themselves; this is where they start.
ALTER TABLE stream_settings
    ADD COLUMN caption_size text NOT NULL DEFAULT 'm' CHECK (caption_size IN ('s','m','l')),
    ADD COLUMN caption_x double precision NOT NULL DEFAULT 0.5 CHECK (caption_x BETWEEN 0 AND 1),
    ADD COLUMN caption_y double precision NOT NULL DEFAULT 0.85 CHECK (caption_y BETWEEN 0 AND 1);
