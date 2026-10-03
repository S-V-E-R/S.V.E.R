-- Module 3 platform reports: chat messages and live streams join the Module 2 report targets.
-- A chat report keeps its own snapshot, so the message body outlives the seven-day chat expiry.
ALTER TABLE reports DROP CONSTRAINT reports_target_type_check;
ALTER TABLE reports ADD CONSTRAINT reports_target_type_check
    CHECK (target_type IN ('profile', 'wall_post', 'wall_reply', 'fan_art', 'setup_photo', 'chat_message', 'live_stream'));
