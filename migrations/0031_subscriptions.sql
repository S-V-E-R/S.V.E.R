-- Module 6 Support, part 2 (docs/SUPPORT.md "Subscriptions"): tiers, card and Valor months, gifts,
-- badges, subscriber emotes and subscriber-only chat. Money still moves only through the ledger.

-- One row per viewer per channel. Benefits run while paid_through is in the future: each paid card
-- invoice extends it to the end of the billed period; a Valor month or a gift adds one month.
-- `months` counts paid months for the subscriber badge.
CREATE TABLE channel_subs (
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    tier smallint NOT NULL CHECK (tier BETWEEN 1 AND 3),
    paid_through timestamptz NOT NULL,
    months integer NOT NULL DEFAULT 0 CHECK (months >= 0),
    stripe_subscription text UNIQUE,
    cancel_at_period_end boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (channel_id, user_id),
    CHECK (channel_id <> user_id)
);
CREATE INDEX channel_subs_user ON channel_subs (user_id);

-- Every paid card subscription invoice, so a refund or dispute can reverse the streamer's share.
CREATE TABLE sub_invoices (
    id text PRIMARY KEY,
    channel_id text NOT NULL,
    user_id text,
    tier smallint NOT NULL,
    payment_intent text UNIQUE,
    amount_cents integer NOT NULL CHECK (amount_cents >= 0),
    share_tenths bigint NOT NULL CHECK (share_tenths >= 0),
    created_at timestamptz NOT NULL DEFAULT now()
);

-- Checkouts now also start card subscriptions and pay for gifts; `detail` holds the channel, tier,
-- chosen gift recipients and the streamer's share.
ALTER TABLE checkout_sessions DROP CONSTRAINT checkout_sessions_kind_check;
ALTER TABLE checkout_sessions ADD CONSTRAINT checkout_sessions_kind_check CHECK (kind IN ('valor', 'sub', 'gift'));
ALTER TABLE checkout_sessions ADD COLUMN detail jsonb NOT NULL DEFAULT '{}';

-- Viewers can turn off receiving gift subs (on by default).
ALTER TABLE users ADD COLUMN allow_gifts boolean NOT NULL DEFAULT true;
-- Owners and moderators can limit chat to subscribers.
ALTER TABLE chat_settings ADD COLUMN subs_only boolean NOT NULL DEFAULT false;
-- Subscriber emotes: 5 per tier on top of the 10 open ones (NULL tier).
ALTER TABLE channel_emotes ADD COLUMN tier smallint CHECK (tier BETWEEN 1 AND 3);
