-- CrowdSync "Creator and developer friendly", Section 3 (docs/CROWDSYNC.md): the catalog of games
-- and boards at /crowdsync.

-- One listing per developer app. `content` is what the owner last sent in (name, summary,
-- description, link, icon, screenshots and a board, or null for a game that sets its own);
-- `approved` is the copy staff last approved, which is all the public sees. A change waits in
-- `content` while `approved` stays up.
CREATE TABLE catalog_listings (
    app_id text PRIMARY KEY REFERENCES dev_apps(id) ON DELETE CASCADE,
    slug text NOT NULL UNIQUE,
    content jsonb NOT NULL,
    approved jsonb,
    status text NOT NULL CHECK (status IN ('PENDING','APPROVED','REJECTED','REMOVED')),
    -- Why staff rejected or removed it, shown to the owner.
    reason text,
    submitted_at timestamptz NOT NULL DEFAULT now(),
    -- First approval: the catalog's "newest first" order.
    approved_at timestamptz
);
CREATE INDEX catalog_listings_public ON catalog_listings (approved_at DESC) WHERE approved IS NOT NULL;

-- Which channels added a listing's board, kept only to count each channel once.
CREATE TABLE catalog_installs (
    app_id text NOT NULL REFERENCES catalog_listings(app_id) ON DELETE CASCADE,
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    installed_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (app_id, channel_id)
);
