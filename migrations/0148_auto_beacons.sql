-- Auto Beacons (docs/BEACONS.md "Sources"): clips the streamer approves become published
-- Beacons. Off by default; Creator Studio → Videos & clips switches it on.
ALTER TABLE video_settings ADD COLUMN auto_beacons boolean NOT NULL DEFAULT false;
