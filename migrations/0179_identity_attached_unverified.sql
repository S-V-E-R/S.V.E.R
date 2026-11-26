-- A sign-in provider attached while the account email was unverified may belong to whoever signed
-- up with someone else's address. Verifying the address does not vouch for it, so a password reset
-- (proof of address ownership) still removes it (docs/LOGIN.md).
ALTER TABLE identities ADD COLUMN attached_unverified boolean NOT NULL DEFAULT false;
UPDATE identities i SET attached_unverified=true FROM users u WHERE u.id=i.user_id AND NOT u.email_verified;
