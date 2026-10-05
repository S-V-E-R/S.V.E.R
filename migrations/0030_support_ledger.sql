-- Module 6 Support, part 1 (docs/SUPPORT.md): the double-entry ledger, Stripe webhooks, payout
-- accounts, Valor purchases and tributes. Balances are always derived from ledger entries.

-- One money movement. `reference` is its idempotency key (for example a Stripe event or a
-- checkout session), so a retried or replayed event can never post twice.
CREATE TABLE ledger_transactions (
    id text PRIMARY KEY,
    kind text NOT NULL,
    reference text NOT NULL UNIQUE,
    detail jsonb NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now()
);
-- Units: 'valor' (whole Valor) and 'usd' (tenths of a cent; Valor earnings accrue at that grain).
-- Accounts are text keys, for example 'valor:<user>', 'usd:earnings:<user>', 'usd:stripe'.
CREATE TABLE ledger_entries (
    id bigserial PRIMARY KEY,
    transaction_id text NOT NULL REFERENCES ledger_transactions(id),
    account text NOT NULL,
    unit text NOT NULL CHECK (unit IN ('valor', 'usd')),
    amount bigint NOT NULL CHECK (amount <> 0)
);
CREATE INDEX ledger_entries_account ON ledger_entries (account, unit);
CREATE INDEX ledger_entries_transaction ON ledger_entries (transaction_id);
-- Double entry: every transaction's entries sum to zero in each unit, checked at commit.
CREATE FUNCTION ledger_balanced() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF EXISTS (SELECT 1 FROM ledger_entries WHERE transaction_id = NEW.transaction_id
               GROUP BY unit HAVING sum(amount) <> 0) THEN
        RAISE EXCEPTION 'ledger transaction % does not balance', NEW.transaction_id;
    END IF;
    RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER ledger_entries_balanced AFTER INSERT ON ledger_entries
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION ledger_balanced();
-- Entries are history: never edited or deleted (corrections post reversing entries).
CREATE FUNCTION ledger_immutable() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'ledger entries are append-only';
END $$;
CREATE TRIGGER ledger_entries_immutable BEFORE UPDATE OR DELETE ON ledger_entries
    FOR EACH ROW EXECUTE FUNCTION ledger_immutable();

-- Every Stripe webhook event, stored before it is processed (idempotent by event id).
CREATE TABLE stripe_events (
    id text PRIMARY KEY,
    type text NOT NULL,
    payload jsonb NOT NULL,
    received_at timestamptz NOT NULL DEFAULT now(),
    processed_at timestamptz,
    error text
);

-- A creator's Stripe Connect Express account. Guardian accounts (ages 13 to 17) are owned and
-- onboarded by a parent or legal guardian, as Stripe requires.
CREATE TABLE payout_accounts (
    user_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    stripe_account text NOT NULL UNIQUE,
    guardian boolean NOT NULL DEFAULT false,
    details_submitted boolean NOT NULL DEFAULT false,
    payouts_enabled boolean NOT NULL DEFAULT false,
    requirements jsonb NOT NULL DEFAULT '[]',
    imported boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- A Checkout Session S.V.E.R created, so a webhook can be matched to its buyer and pack.
CREATE TABLE checkout_sessions (
    id text PRIMARY KEY,
    user_id text REFERENCES users(id) ON DELETE SET NULL,
    kind text NOT NULL CHECK (kind IN ('valor')),
    amount_cents integer NOT NULL CHECK (amount_cents > 0),
    valor integer,
    payment_intent text UNIQUE,
    status text NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'paid', 'expired')),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX checkout_sessions_minor_cap ON checkout_sessions (user_id, created_at) WHERE status = 'paid';
-- A parent or guardian confirmed they are the cardholder for a buyer aged 13 to 17.
CREATE TABLE guardian_consents (
    user_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    confirmed_at timestamptz NOT NULL DEFAULT now()
);

-- A tribute is a chat message with Valor attached.
ALTER TABLE chat_messages ADD COLUMN tribute integer CHECK (tribute IS NULL OR tribute >= 10);
