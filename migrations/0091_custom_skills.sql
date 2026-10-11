-- Streamer-uploaded Skills (docs/CROWDSYNC.md "Custom Skills"; Joe's decision October 10, 2026:
-- fixed price tiers). Up to 10 per channel, each with an uploaded image and/or sound, priced only
-- at S.V.E.R's existing Skill price points. Earnings are the same 0.8 cents per Valor.
CREATE TABLE channel_skills (
    id text PRIMARY KEY,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 30),
    valor integer NOT NULL CHECK (valor IN (30, 50, 75, 100, 150, 200, 300)),
    image_key text,
    audio_key text,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX channel_skills_channel ON channel_skills (channel_id, created_at);
