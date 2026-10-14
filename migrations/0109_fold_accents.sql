-- Search matches "pokemon" to "Pokémon": channel, category and Beacon names compare without accents.
-- Built-in normalize() splits letters from their accents, which are then dropped (no extension needed).
CREATE FUNCTION fold_accents(text) RETURNS text LANGUAGE sql IMMUTABLE PARALLEL SAFE
RETURN regexp_replace(normalize($1, NFD), '[̀-ͯ]', '', 'g');
