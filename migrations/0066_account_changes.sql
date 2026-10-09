-- Account changes while signed in (docs/LOGIN.md "Account changes"): an email change waits for
-- a link sent to the new address; the challenge payload holds that address.
ALTER TABLE challenges DROP CONSTRAINT challenges_kind_check;
ALTER TABLE challenges ADD CONSTRAINT challenges_kind_check CHECK (kind IN ('verify', 'reset', 'mfa', 'setup', 'email'));
