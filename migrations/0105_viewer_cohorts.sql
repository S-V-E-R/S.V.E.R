-- Viewer integrity cohorts (docs/LIVE_STREAMS.md "Signals"): a keyed hash of the browser build
-- (User-Agent, re-keyed every 30 days like the network hashes; never the raw string) and whether
-- the session joined as part of a cohort: several sessions from one network with one browser
-- build starting within seconds of each other. One soft signal; it never excludes on its own.
ALTER TABLE playback_leases ADD COLUMN ua_hash text, ADD COLUMN cohort boolean NOT NULL DEFAULT false;
