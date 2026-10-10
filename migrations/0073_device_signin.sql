-- Developer platform: device sign-in at sver.tv/go (RFC 8628). The app shows a 6-character code;
-- the person approves it signed in. Codes last 10 minutes and are used once.
CREATE TABLE oauth_devices (
    device_hash text PRIMARY KEY,
    user_code text NOT NULL UNIQUE,
    app_id text NOT NULL REFERENCES dev_apps(id) ON DELETE CASCADE,
    scopes text[] NOT NULL,
    -- The device's network in plain words (provider and country), shown on the approval screen.
    network text,
    created_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    last_poll_at timestamptz,
    user_id text REFERENCES users(id) ON DELETE CASCADE,
    decision text CHECK (decision IN ('approved', 'denied'))
);
CREATE INDEX oauth_devices_expiry ON oauth_devices (expires_at);
