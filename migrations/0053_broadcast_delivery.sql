-- Module 3 transport policy (docs/LIVE_STREAMS.md "Playback and capacity"): a broadcast moves from
-- direct WebRTC to the CDN after 30 seconds over the measured WebRTC limit, and back after 120
-- seconds at or under 70% of it.
ALTER TABLE broadcasts
    ADD COLUMN delivery text NOT NULL DEFAULT 'webrtc' CHECK (delivery IN ('webrtc', 'cdn')),
    ADD COLUMN delivery_over_since timestamptz,
    ADD COLUMN delivery_under_since timestamptz;
