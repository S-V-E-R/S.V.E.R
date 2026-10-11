-- App icons (docs/DEVELOPER_PLATFORM.md §1): a developer app's square icon, shown on the consent
-- screen, sver.tv/go, Connected apps and the staff list. Stored through the image pipeline
-- (fingerprinted, Take It Down covered); staff suspend an app whose icon breaks the rules.
ALTER TABLE dev_apps ADD COLUMN icon_key text;
