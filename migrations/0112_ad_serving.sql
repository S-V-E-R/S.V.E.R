-- Ad network step 2 (docs/ADVERTISERS.md): streamer ad breaks, ads handed to viewers with
-- single-use tickets, and the verified or rejected outcome of each.

-- Ads are off for a channel until the streamer turns them on.
CREATE TABLE channel_ads (
    channel_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    enabled boolean NOT NULL DEFAULT false,
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- A break the streamer started; viewers of that broadcast see ads until ends_at.
CREATE TABLE ad_breaks (
    id text PRIMARY KEY,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    broadcast_id text NOT NULL,
    starts_at timestamptz NOT NULL DEFAULT now(),
    ends_at timestamptz NOT NULL
);
CREATE INDEX ad_breaks_channel ON ad_breaks(channel_id, ends_at DESC);

-- One row per ad handed to a viewer. Its id is the single-use impression ticket: it is decided
-- once, as verified (billed, counts toward the campaign) or rejected with the reason.
CREATE TABLE ad_serves (
    id text PRIMARY KEY,
    campaign_id text NOT NULL REFERENCES ad_campaigns(id) ON DELETE CASCADE,
    creative_id text NOT NULL,
    placement text NOT NULL CHECK (placement IN ('break', 'feed', 'site')),
    viewer_key text NOT NULL,
    user_id text REFERENCES users(id) ON DELETE SET NULL,
    channel_id text REFERENCES users(id) ON DELETE SET NULL,
    break_id text REFERENCES ad_breaks(id) ON DELETE SET NULL,
    served_at timestamptz NOT NULL DEFAULT now(),
    outcome text CHECK (outcome IN ('verified', 'rejected')),
    reason text,
    decided_at timestamptz,
    clicked_at timestamptz
);
CREATE INDEX ad_serves_campaign ON ad_serves(campaign_id, outcome, decided_at);
CREATE INDEX ad_serves_viewer ON ad_serves(viewer_key, campaign_id, served_at DESC);

-- Eligible viewers who could have been shown an ad, per day and placement: the measured forecast.
CREATE TABLE ad_opportunities (
    day date NOT NULL,
    placement text NOT NULL CHECK (placement IN ('break', 'feed', 'site')),
    count bigint NOT NULL DEFAULT 0,
    PRIMARY KEY (day, placement)
);
