-- Every /wow submit on the home server and what Chromely said (docs/DISCORD_BOT.md "Community");
-- Chromely holds the runs themselves. The legacy bot's run submissions are imported here.
CREATE TABLE discord_wow_runs (
    id bigserial PRIMARY KEY,
    discord_user text NOT NULL,
    character text NOT NULL,
    realm text NOT NULL,
    seconds double precision NOT NULL,
    proof_url text,
    status text NOT NULL CHECK (status IN ('accepted', 'rejected', 'error')),
    reason text,
    submitted_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX discord_wow_runs_user ON discord_wow_runs(discord_user, submitted_at DESC);
