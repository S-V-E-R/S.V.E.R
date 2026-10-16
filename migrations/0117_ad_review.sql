-- Ad network review fixes (docs/ADVERTISERS.md).
-- A campaign under an open chargeback pauses ('disputed'); a refund made outside the campaign's own
-- flow is remembered so the job refunds only the rest; a refund Stripe definitely declined waits
-- for staff instead of retrying forever.
ALTER TABLE ad_campaigns DROP CONSTRAINT ad_campaigns_status_check;
ALTER TABLE ad_campaigns ADD CONSTRAINT ad_campaigns_status_check
    CHECK (status IN ('checkout', 'expired', 'paid', 'running', 'disputed', 'delivered', 'refund_due', 'cancelled', 'withdrawn'));
ALTER TABLE ad_campaigns
    ADD COLUMN outside_cents integer NOT NULL DEFAULT 0,
    ADD COLUMN refund_error text;

-- The measured forecast counts each eligible viewer once a day per placement, not each request.
CREATE TABLE ad_reach (
    day date NOT NULL,
    placement text NOT NULL,
    viewer_key text NOT NULL,
    PRIMARY KEY (day, placement, viewer_key)
);
-- Request counts from before this change aren't comparable; measurement starts again.
DELETE FROM ad_opportunities;

-- Streamers' and viewers' shares accumulate in hundredths of a tenth of a cent, so a share smaller
-- than a tenth of a cent per impression is paid as it adds up instead of rounding to nothing.
CREATE TABLE ad_share_carry (
    account text PRIMARY KEY,
    hundredths bigint NOT NULL DEFAULT 0
);

-- Feed and site ads need an account that watched for real recently (a counted live session or
-- Beacon view); these find it by viewer.
CREATE INDEX playback_leases_viewer ON playback_leases (viewer_key, last_beat_at);
CREATE INDEX beacon_playback_viewer ON beacon_playback (viewer_key, day) WHERE counted;
