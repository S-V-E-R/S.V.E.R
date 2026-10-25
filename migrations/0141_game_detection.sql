-- Game detection (docs/LIVE_STREAMS.md "Game detection"): opt-in, suggest-only.
ALTER TABLE stream_settings
    ADD COLUMN game_detection boolean NOT NULL DEFAULT false,
    -- When the streamer last chose a category: stills soon after are trusted as examples of it.
    ADD COLUMN category_at timestamptz,
    -- The game area of the channel's layout as fractions of the frame [x0,y0,x1,y1].
    ADD COLUMN detect_area real[] CHECK (detect_area IS NULL OR cardinality(detect_area) = 4);

ALTER TABLE broadcasts
    ADD COLUMN detect_at timestamptz,
    ADD COLUMN detect_guess text REFERENCES stream_categories(id) ON DELETE SET NULL,
    ADD COLUMN detect_streak integer NOT NULL DEFAULT 0,
    ADD COLUMN detect_dismissed text[] NOT NULL DEFAULT '{}';

-- One CLIP image embedding (512 little-endian f32) of a still, labeled with the category the
-- streamer had chosen. Removed when the streamer switches detection off.
CREATE TABLE game_examples (
    id bigserial PRIMARY KEY,
    category_id text NOT NULL REFERENCES stream_categories(id) ON DELETE CASCADE,
    owner_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    embedding bytea NOT NULL CHECK (length(embedding) = 2048),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX game_examples_owner_category ON game_examples (owner_id, category_id, created_at);
