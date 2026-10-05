-- Run only against the isolated legacy backup restore. Keep the resulting mapping outside the repo.
SELECT jsonb_object_agg(id::text,lower(slug::text)) FROM "Faction";
