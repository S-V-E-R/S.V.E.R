-- A public username on a share link (?ref=), stored only so we can see which links
-- lead to new accounts. It never changes discovery, ranking or who gets seen.
CREATE TABLE signup_referrals (
    user_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    referrer_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    CHECK (user_id <> referrer_id)
);
CREATE INDEX signup_referrals_referrer ON signup_referrals (referrer_id);
