-- A bandwidth test publish (`bandwidthtest=1`) checks the key and the connection without going
-- live: no broadcast, so nothing plays it, and the reconciler disconnects it at test_until.
ALTER TABLE stream_publishers ADD COLUMN test_until timestamptz;
