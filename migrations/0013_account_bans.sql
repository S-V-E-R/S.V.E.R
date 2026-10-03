-- Module 3 platform account bans (approved by Joe, October 3, 2026). A ban is separate from
-- strikes: it restricts the account like the strongest restriction (channel hidden, no public
-- actions, stream revoked), revokes every session and leaves only security, standing and appeal
-- routes writable. A timed ban ends by database time; `until` NULL means indefinite.
-- 0011 and 0012 belong to the setup-parts work.
CREATE TABLE account_bans (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    reason text NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 500),
    message_to_user text NOT NULL DEFAULT '',
    staff_note text NOT NULL DEFAULT '',
    issued_by text NOT NULL,
    issued_at timestamptz NOT NULL DEFAULT now(),
    until timestamptz,
    -- An overturned related strike asks staff to re-review the ban; it never cancels it silently.
    strike_id text REFERENCES strikes(id) ON DELETE SET NULL,
    review_requested_at timestamptz,
    status text NOT NULL DEFAULT 'ACTIVE' CHECK (status IN ('ACTIVE', 'LIFTED', 'OVERTURNED')),
    ended_at timestamptz,
    ended_by text,
    CHECK (until IS NULL OR until > issued_at),
    CHECK ((status = 'ACTIVE') = (ended_at IS NULL))
);
CREATE INDEX account_bans_user ON account_bans (user_id, issued_at DESC);

-- One appeal per strike or per ban, with the same workflow and limits.
ALTER TABLE appeals ALTER COLUMN strike_id DROP NOT NULL;
ALTER TABLE appeals ADD COLUMN ban_id text UNIQUE REFERENCES account_bans(id) ON DELETE CASCADE;
ALTER TABLE appeals ADD CONSTRAINT appeals_one_target CHECK ((strike_id IS NULL) <> (ban_id IS NULL));
