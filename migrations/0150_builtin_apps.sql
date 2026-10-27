-- Apps S.V.E.R makes itself have no owner: sver.tv/go says "Made by S.V.E.R", and no one can
-- change or delete them in Settings, since every owner check is owner_id = the signed-in person.
-- Staff can still suspend one (docs/DEVELOPER_PLATFORM.md "Desktop stream key").
ALTER TABLE dev_apps ALTER COLUMN owner_id DROP NOT NULL;
INSERT INTO dev_apps(id, owner_id, name, redirect_uris)
VALUES ('sver-desktop', NULL, 'S.V.E.R Desktop', ARRAY['http://127.0.0.1/cb'])
ON CONFLICT (id) DO NOTHING;
