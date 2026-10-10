-- Staff console operations (docs/ADMIN.md "Operations"): emergency switches turn a risky feature off
-- and on with no deploy; the site banner is one message across the top of every page. Every change
-- is audited in moderation_actions.
CREATE TABLE feature_switches (
    name text PRIMARY KEY,
    off boolean NOT NULL DEFAULT false,
    changed_by text REFERENCES users(id) ON DELETE SET NULL,
    changed_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE site_banner (
    id smallint PRIMARY KEY CHECK (id = 1),
    message text NOT NULL CHECK (char_length(message) BETWEEN 1 AND 280),
    ends_at timestamptz,
    updated_by text REFERENCES users(id) ON DELETE SET NULL,
    updated_at timestamptz NOT NULL DEFAULT now()
);
