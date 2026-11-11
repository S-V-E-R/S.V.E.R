-- Phone push for the S.V.E.R app. Browser Web Push stays on push_subscriptions (VAPID).
-- Expo tokens are a different credential, sealed the same way, and follow the same push switches.
-- dm and light already use notification_type_settings; the original check predated them.
ALTER TABLE notification_type_settings DROP CONSTRAINT notification_type_settings_kind_check;
ALTER TABLE notification_type_settings ADD CONSTRAINT notification_type_settings_kind_check
    CHECK (kind IN ('guild_application', 'guild_decision', 'guild_invite', 'squad_invite', 'dm', 'light'));

CREATE TABLE expo_devices (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX expo_devices_user ON expo_devices (user_id);

CREATE TABLE expo_push_jobs (
    id text PRIMARY KEY,
    device_id text NOT NULL REFERENCES expo_devices(id) ON DELETE CASCADE,
    broadcast_id text REFERENCES broadcasts(id) ON DELETE CASCADE,
    notification_id text REFERENCES notifications(id) ON DELETE CASCADE,
    attempts integer NOT NULL DEFAULT 0,
    available_at timestamptz NOT NULL DEFAULT now(),
    created_at timestamptz NOT NULL DEFAULT now(),
    CONSTRAINT expo_push_job_target CHECK ((broadcast_id IS NULL) <> (notification_id IS NULL))
);
CREATE UNIQUE INDEX expo_push_broadcast ON expo_push_jobs (device_id, broadcast_id);
CREATE UNIQUE INDEX expo_push_notice ON expo_push_jobs (device_id, notification_id);
CREATE INDEX expo_push_jobs_due ON expo_push_jobs (available_at);
