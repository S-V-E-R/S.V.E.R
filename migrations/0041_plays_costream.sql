-- The one streamer whose co-stream invitations the Plays channel accepts on its own (docs/PLAYS.md).
ALTER TABLE plays_runtime ADD COLUMN costream_host_id text REFERENCES users(id) ON DELETE SET NULL;
