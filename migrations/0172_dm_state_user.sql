-- The Messages badge (GET /api/dms/unread) reads one person's dm_state rows on every page load;
-- the primary key starts with the conversation, so without this it scanned the whole table.
-- No INCLUDE: read_seq changes on every send and read, and indexing it would end HOT updates.
CREATE INDEX dm_state_user ON dm_state (user_id);
