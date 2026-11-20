-- The Messages badge (GET /api/dms/unread) reads one person's dm_state rows on every page load;
-- the primary key starts with the conversation, so without this it scanned the whole table.
CREATE INDEX dm_state_user ON dm_state (user_id) INCLUDE (read_seq, cleared_seq, muted);
