-- S.V.E.R's own network of approved ads (docs/ADVERTISERS.md), build step 1: the rate card,
-- advertisers, ads under review, and prepaid campaigns sold only against forecast inventory.

-- One row per placement. CPM is per 1,000 verified impressions; NULL means not for sale.
-- daily_estimate is staff's starting forecast of verified impressions a day, used until delivery
-- history exists.
CREATE TABLE ad_rate_card (
    placement text PRIMARY KEY CHECK (placement IN ('break', 'feed', 'site')),
    cpm_cents int CHECK (cpm_cents BETWEEN 50 AND 100000),
    min_spend_cents int NOT NULL DEFAULT 2500 CHECK (min_spend_cents BETWEEN 100 AND 10000000),
    daily_estimate int NOT NULL DEFAULT 0 CHECK (daily_estimate BETWEEN 0 AND 100000000),
    updated_at timestamptz NOT NULL DEFAULT now()
);
INSERT INTO ad_rate_card(placement) VALUES ('break'), ('feed'), ('site');

CREATE TABLE advertisers (
    user_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    name text NOT NULL,
    website text NOT NULL,
    country text NOT NULL CHECK (country ~ '^[A-Z]{2}$'),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- An uploaded ad. Staff approve it (for 18+ only, or as teen-safe) or reject it with a reason.
CREATE TABLE ad_creatives (
    id text PRIMARY KEY,
    advertiser_id text NOT NULL REFERENCES advertisers(user_id) ON DELETE CASCADE,
    format text NOT NULL CHECK (format IN ('video_landscape', 'video_portrait', 'image_landscape', 'image_portrait')),
    stored text NOT NULL,
    duration_ms int,
    title text NOT NULL,
    link text NOT NULL,
    status text NOT NULL DEFAULT 'review' CHECK (status IN ('review', 'approved', 'rejected', 'retired')),
    teen_safe boolean NOT NULL DEFAULT false,
    review_note text NOT NULL DEFAULT '',
    reviewed_by text REFERENCES users(id) ON DELETE SET NULL,
    reviewed_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ad_creatives_review ON ad_creatives(created_at) WHERE status = 'review';

-- A prepaid campaign for one placement. Money records outlive the advertiser's account.
-- checkout: holds inventory until hold_until; paid: waits for an approved ad and its start date;
-- running: serving (build step 2); refund_due: owes refund_cents, then becomes `ending`.
CREATE TABLE ad_campaigns (
    id text PRIMARY KEY,
    advertiser_id text REFERENCES advertisers(user_id) ON DELETE SET NULL,
    creative_id text REFERENCES ad_creatives(id) ON DELETE SET NULL,
    name text NOT NULL,
    placement text NOT NULL CHECK (placement IN ('break', 'feed', 'site')),
    genres text[] NOT NULL DEFAULT '{}',
    languages text[] NOT NULL DEFAULT '{}',
    audience text NOT NULL DEFAULT 'all' CHECK (audience IN ('all', 'adult')),
    impressions int NOT NULL CHECK (impressions >= 1000),
    delivered int NOT NULL DEFAULT 0 CHECK (delivered >= 0),
    cpm_cents int NOT NULL,
    amount_cents int NOT NULL CHECK (amount_cents > 0),
    starts_on date NOT NULL,
    ends_on date NOT NULL CHECK (ends_on >= starts_on),
    status text NOT NULL CHECK (status IN ('checkout', 'expired', 'paid', 'running', 'delivered', 'refund_due', 'cancelled', 'withdrawn')),
    ending text CHECK (ending IN ('cancelled', 'withdrawn')),
    refund_cents int NOT NULL DEFAULT 0 CHECK (refund_cents >= 0),
    checkout_id text UNIQUE,
    payment_intent text,
    hold_until timestamptz,
    paid_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX ad_campaigns_inventory ON ad_campaigns(placement, status);
CREATE INDEX ad_campaigns_advertiser ON ad_campaigns(advertiser_id, created_at DESC);

-- Ad sales stay off until the legal and tax items are done (docs/ADVERTISERS.md "Build order").
INSERT INTO feature_switches(name, off) VALUES ('ad_sales', true) ON CONFLICT (name) DO NOTHING;

-- Ad campaigns are paid through the shared Stripe checkout and webhook.
ALTER TABLE checkout_sessions DROP CONSTRAINT checkout_sessions_kind_check,
    ADD CONSTRAINT checkout_sessions_kind_check CHECK (kind IN ('valor', 'sub', 'gift', 'wall', 'platform', 'ad'));
