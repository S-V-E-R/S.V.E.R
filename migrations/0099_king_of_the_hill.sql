-- King of the Hill (docs/FACTIONS.md "King of the Hill"; Joe's decision October 11, 2026): each season
-- has one Hill genre (a neutral one first, picked by the season). Influence there counts double
-- (faction events never stack), and whoever holds it when the season ends gets extra territories.
ALTER TABLE faction_seasons ADD COLUMN hill text;
UPDATE faction_seasons s SET hill = (SELECT g.id FROM faction_genres g ORDER BY (g.home IS NOT NULL), md5(s.number::text || g.id), g.id LIMIT 1) WHERE s.hill IS NULL;
