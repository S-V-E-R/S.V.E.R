-- A token keeps the scopes approved when it was issued (NULL: issued before this, follows the grant).
-- One grant per person and app, so a later approval for more no longer widens earlier tokens.
ALTER TABLE oauth_tokens ADD COLUMN scopes text[];
