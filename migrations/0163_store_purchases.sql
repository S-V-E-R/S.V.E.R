-- Valor bought through the App Store and Google Play (docs/SUPPORT.md "Store packs").

-- A stable, random, non-personal id per account: Apple's appAccountToken and Google's
-- obfuscatedAccountId. The app gets it from the purchase pre-check.
ALTER TABLE users ADD COLUMN store_token uuid NOT NULL DEFAULT gen_random_uuid();
CREATE UNIQUE INDEX users_store_token ON users (store_token);

-- One row per store purchase, written in the same transaction as its ledger credit.
CREATE TABLE store_purchases (
    store text NOT NULL CHECK (store IN ('apple', 'google')),
    store_id text NOT NULL,                 -- Apple transactionId, Play purchaseToken
    user_id text REFERENCES users(id) ON DELETE SET NULL,
    product_id text NOT NULL,
    quantity integer NOT NULL CHECK (quantity > 0),
    valor bigint NOT NULL CHECK (valor > 0),    -- quantity x pack Valor
    cents bigint NOT NULL CHECK (cents >= 0),   -- quantity x pack US list price, for the under-18 cap
    environment text NOT NULL CHECK (environment IN ('production', 'sandbox')),
    -- The refunded share in milliunits (100000 = all of it), as the store reports it.
    refunded_milli integer NOT NULL DEFAULT 0 CHECK (refunded_milli BETWEEN 0 AND 100000),
    refund_seq integer NOT NULL DEFAULT 0,      -- bumps on every refund change; part of its ledger reference
    consumed boolean NOT NULL DEFAULT false,    -- Google only; Apple rows are inserted true
    payload jsonb NOT NULL,                     -- the store API's response
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (store, store_id)
);
CREATE INDEX store_purchases_user ON store_purchases (user_id, created_at);
CREATE INDEX store_purchases_unconsumed ON store_purchases (created_at) WHERE NOT consumed;

-- A purchase the pre-check let through. It counts toward the under-18 cap for 30 minutes, or until
-- its purchase is credited (the role open Stripe checkouts play).
CREATE TABLE store_intents (
    id bigserial PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    cents bigint NOT NULL CHECK (cents > 0),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX store_intents_user ON store_intents (user_id, created_at);
