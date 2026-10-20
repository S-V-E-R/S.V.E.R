-- A payout whose Stripe result is unknown stays pending and is resumed by the jobs loop
-- (payouts::resume); attempted_at keeps two resumes from working the same run at once.
ALTER TABLE payout_runs ADD COLUMN attempted_at timestamptz;
CREATE INDEX payout_runs_pending ON payout_runs (created_at) WHERE status = 'pending';
