-- Automatic clip suggestions (docs/VODS_CLIPS.md "Automatic clip suggestions"): room and loud
-- moments become clips waiting for the streamer's approval. On by default; Studio switches it off.
ALTER TABLE video_settings ADD COLUMN auto_clips boolean NOT NULL DEFAULT true;
