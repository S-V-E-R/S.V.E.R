-- Channel trailer (docs/PROFILES.md "Channel trailer"): one intro video per channel, either one of
-- the owner's own public clips or Highlights ('sver', media id = the video id) or a YouTube video.
ALTER TABLE profiles
    ADD COLUMN trailer_provider text CHECK (trailer_provider IN ('youtube', 'sver')),
    ADD COLUMN trailer_media_id text,
    ADD COLUMN trailer_url text,
    ADD COLUMN trailer_title text,
    ADD COLUMN trailer_thumb_key text,
    ADD COLUMN trailer_updated_at timestamptz,
    ADD CONSTRAINT profiles_trailer_complete CHECK (trailer_provider IS NULL OR (trailer_media_id IS NOT NULL AND trailer_url IS NOT NULL));
