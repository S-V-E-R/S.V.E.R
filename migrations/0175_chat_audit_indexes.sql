-- Chat evidence on timeouts and bans is stripped 90 days on (safety tick): find only rows still holding it.
CREATE INDEX channel_moderation_log_evidence ON channel_moderation_log (created_at) WHERE detail ? 'chat';
-- A MAGNet room's earlier merged windows read a channel's lines (its own and shared co-stream ones)
-- by time.
CREATE INDEX chat_messages_window ON chat_messages (channel_id, created_at) WHERE deleted_at IS NULL;
-- Reading or sending a DM drops that conversation's queued pushes (dms::notice_read); these also
-- serve the cascade when a notification is deleted.
CREATE INDEX push_jobs_notification ON push_jobs (notification_id) WHERE notification_id IS NOT NULL;
CREATE INDEX expo_push_jobs_notification ON expo_push_jobs (notification_id) WHERE notification_id IS NOT NULL;
