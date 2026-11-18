-- Discovery's per-stream badges (first stream, returning, restarted) look up a channel's earlier
-- broadcasts by owner and start time; until now only partial indexes covered owner_id.
CREATE INDEX broadcasts_owner_started ON broadcasts (owner_id, started_at);

-- Paydays are now noon Eastern in standard time too (17:00 UTC, not 16:00). Mark runs already
-- made under that day's Eastern noon as well, so a payday paid at the old time doesn't run a
-- second time. The old keys stay, so a rolled-back binary doesn't re-run them either.
INSERT INTO support_runs (kind, period, ran_at)
SELECT kind, (date_trunc('day', period AT TIME ZONE 'America/New_York') + interval '12 hours') AT TIME ZONE 'America/New_York', ran_at
FROM support_runs WHERE kind = 'payday'
ON CONFLICT DO NOTHING;
