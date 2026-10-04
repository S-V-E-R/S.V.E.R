-- Removal notices must survive a mail-service outage longer than the login retry budget.
ALTER TABLE mail_jobs ADD COLUMN retry_until_expiry boolean NOT NULL DEFAULT false;
UPDATE mail_jobs m SET retry_until_expiry=true,
    expires_at=greatest(m.expires_at,m.created_at+interval '7 days')
WHERE EXISTS(SELECT 1 FROM take_down_deliveries d WHERE d.mail_id=m.id);

-- Keep provider acceptance/failure records for the case's full retention period, even after
-- mail/push jobs and expired subscriptions have been removed. mail_id is the existing job key.
ALTER TABLE take_down_deliveries
    ADD COLUMN channel text NOT NULL DEFAULT 'email' CHECK(channel IN ('email','push')),
    ADD COLUMN state text NOT NULL DEFAULT 'queued'
        CHECK(state IN ('queued','retrying','accepted','failed','expired')),
    ADD COLUMN last_attempt_at timestamptz;
UPDATE take_down_deliveries SET state=CASE
    WHEN sent_at IS NOT NULL THEN 'accepted'
    WHEN NOT EXISTS(SELECT 1 FROM mail_jobs m WHERE m.id=mail_id) THEN 'expired'
    WHEN attempts>0 THEN 'retrying' ELSE 'queued' END;
CREATE INDEX take_down_deliveries_request ON take_down_deliveries(request_id,queued_at);
