-- Developer platform §2 (docs/DEVELOPER_PLATFORM.md "Live events"): an outbox written in the same
-- transaction as the change, fanned out to /api/events sockets. Kept 10 minutes, enough for a
-- reconnecting client to ask for everything after its last event ID from the past 5 minutes.
CREATE TABLE events (
    id bigserial PRIMARY KEY,
    topic text NOT NULL,
    data jsonb NOT NULL,
    at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX events_at ON events (at);
