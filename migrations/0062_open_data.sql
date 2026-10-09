-- Open data (docs/CHANNEL_ADDITIONS.md): weekly and monthly platform figures written nightly, and
-- the broadcasts the fair rotation put in first place (the fairness promise, measured).
CREATE TABLE public_stats (
    period text NOT NULL,
    metric text NOT NULL,
    value double precision NOT NULL,
    computed_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (period, metric)
);
CREATE TABLE rotation_firsts (
    broadcast_id text PRIMARY KEY REFERENCES broadcasts(id) ON DELETE CASCADE,
    at timestamptz NOT NULL DEFAULT now()
);
