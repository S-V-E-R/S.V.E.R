-- Platform supporters and Founders (docs/SUPPORTERS.md): a monthly subscription to S.V.E.R itself
-- (Wolfsworn $5, Greycloak $25). Cosmetic only; all of the money goes to the platform.
CREATE TABLE platform_subscriptions (
    user_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    -- 1 Wolfsworn, 2 Greycloak: what the paid period gives.
    tier smallint NOT NULL CHECK (tier IN (1, 2)),
    -- A downgrade waiting for the next renewal.
    pending_tier smallint CHECK (pending_tier IN (1, 2)),
    stripe_subscription text UNIQUE,
    paid_through timestamptz NOT NULL,
    cancel_at_period_end boolean NOT NULL DEFAULT false,
    -- Months supported in total, across tiers and breaks.
    months integer NOT NULL DEFAULT 0,
    -- The first supported month, for "Supporting since".
    since timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- Every paid platform invoice: credits the period, and is how a refund or dispute finds the
-- payment (and a Founder spot it claimed).
CREATE TABLE platform_invoices (
    id text PRIMARY KEY,
    user_id text NOT NULL,
    tier smallint NOT NULL,
    payment_intent text UNIQUE,
    amount_cents integer NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

-- Numbered Founder marks, #1 to #500, claimed by the first paid Greycloak invoice of an account.
-- A refund or lost dispute of that invoice deletes the row, freeing the number.
CREATE TABLE founders (
    number integer PRIMARY KEY CHECK (number BETWEEN 1 AND 500),
    user_id text NOT NULL UNIQUE REFERENCES users(id) ON DELETE CASCADE,
    invoice_id text NOT NULL REFERENCES platform_invoices(id),
    claimed_at timestamptz NOT NULL DEFAULT now(),
    hidden boolean NOT NULL DEFAULT false
);

ALTER TABLE checkout_sessions DROP CONSTRAINT checkout_sessions_kind_check;
ALTER TABLE checkout_sessions ADD CONSTRAINT checkout_sessions_kind_check CHECK (kind IN ('valor', 'sub', 'gift', 'wall', 'platform'));
