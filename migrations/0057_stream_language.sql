-- Stream language (docs/CHANNEL_ADDITIONS.md): an ISO 639-1 code or 'other', saved on the channel,
-- and the viewer's "Languages I watch in" with "Only show streams in my languages" for home.
ALTER TABLE stream_settings ADD COLUMN language text CHECK (language ~ '^([a-z]{2}|other)$');
ALTER TABLE users
    ADD COLUMN languages text[] NOT NULL DEFAULT '{}',
    ADD COLUMN only_my_languages boolean NOT NULL DEFAULT false;
