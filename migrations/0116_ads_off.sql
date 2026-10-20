-- Viewers can turn ads off everywhere (Joe, October 16, 2026; docs/ADS.md "Turning ads off").
-- An account with ads off is never served an ad and earns no Ad Valor.
ALTER TABLE users ADD COLUMN ads_off boolean NOT NULL DEFAULT false;
