-- Legacy deletions remain inactive without entering the new 14-day erasure queue.
ALTER TABLE users ADD COLUMN legacy_deletion_hold boolean NOT NULL DEFAULT false;
ALTER TABLE users ADD CHECK (NOT legacy_deletion_hold OR deleted_at IS NOT NULL);

-- Preserve fields owned by later modules without exposing them through Login APIs.
CREATE TABLE legacy_account_data (
    user_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    account jsonb NOT NULL,
    profile jsonb,
    identities jsonb NOT NULL DEFAULT '[]',
    mfa_exception text
);
