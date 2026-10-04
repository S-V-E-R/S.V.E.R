-- Module 3 go-live alerts (docs/LIVE_STREAMS.md "Go-live alerts"). A broadcast alerts once, when it
-- first reaches LIVE; a reconnect keeps the same broadcast row, so it never alerts twice.
ALTER TABLE follows ADD COLUMN alerts boolean NOT NULL DEFAULT true;
ALTER TABLE broadcasts ADD COLUMN alert_state text CHECK (alert_state IN ('sent', 'throttled', 'skipped'));
-- Broadcasts that exist before this release never alert.
UPDATE broadcasts SET alert_state = 'skipped';
CREATE INDEX broadcasts_alert_pending ON broadcasts (state) WHERE alert_state IS NULL;
CREATE INDEX broadcasts_alert_sent ON broadcasts (owner_id, started_at DESC) WHERE alert_state = 'sent';

-- No row means the defaults: in-site and push on, email off (opt-in).
CREATE TABLE notification_settings (
    user_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    site boolean NOT NULL DEFAULT true,
    push boolean NOT NULL DEFAULT true,
    email boolean NOT NULL DEFAULT false
);
-- In-site notifications, kept 30 days.
CREATE TABLE notifications (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('live')),
    channel_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    broadcast_id text NOT NULL REFERENCES broadcasts(id) ON DELETE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    read_at timestamptz,
    UNIQUE (user_id, broadcast_id)
);
CREATE INDEX notifications_recent ON notifications (user_id, created_at DESC);
-- Viewer browser push (staff push keeps its own tables). The subscription is encrypted.
CREATE TABLE push_subscriptions (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    subscription text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX push_subscriptions_user ON push_subscriptions (user_id);
CREATE TABLE push_jobs (
    id text PRIMARY KEY,
    subscription_id text NOT NULL REFERENCES push_subscriptions(id) ON DELETE CASCADE,
    broadcast_id text NOT NULL REFERENCES broadcasts(id) ON DELETE CASCADE,
    attempts integer NOT NULL DEFAULT 0,
    available_at timestamptz NOT NULL DEFAULT now(),
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (subscription_id, broadcast_id)
);
CREATE INDEX push_jobs_due ON push_jobs (available_at);
