-- CrowdSync "Connect to S.V.E.R" (docs/CROWDSYNC.md, Section 2): the "May set my board" choice
-- made on the approval screen, carried from the approval to the grant it creates.
ALTER TABLE oauth_devices ADD COLUMN may_define boolean NOT NULL DEFAULT false;
ALTER TABLE oauth_codes ADD COLUMN may_define boolean NOT NULL DEFAULT false;
