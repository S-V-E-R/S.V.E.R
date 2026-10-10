-- Username claims for inactive accounts (Joe's decision October 10, 2026).
CREATE TABLE username_claims (
    id text PRIMARY KEY,
    handle text NOT NULL,
    holder_id text REFERENCES users(id) ON DELETE SET NULL,
    requester_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status text NOT NULL DEFAULT 'WAITING' CHECK (status IN ('WAITING','KEPT','READY','RELEASED','REJECTED','WITHDRAWN')),
    created_at timestamptz NOT NULL DEFAULT now(),
    decided_at timestamptz,
    decided_by text REFERENCES users(id) ON DELETE SET NULL,
    reason text
);
CREATE UNIQUE INDEX username_claims_open_handle ON username_claims (lower(handle)) WHERE status IN ('WAITING','READY');
CREATE UNIQUE INDEX username_claims_open_requester ON username_claims (requester_id) WHERE status IN ('WAITING','READY');
ALTER TABLE username_history DROP CONSTRAINT username_history_reason_check;
ALTER TABLE username_history ADD CONSTRAINT username_history_reason_check CHECK (reason IN ('rename', 'revert', 'import', 'staff_reset', 'claim', 'released'));
