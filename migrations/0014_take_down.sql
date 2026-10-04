-- Anonymous removal requests retain encrypted contact details independently of account erasure.
ALTER TABLE mail_jobs ALTER COLUMN user_id DROP NOT NULL;
ALTER TABLE media_objects ADD COLUMN fingerprinted boolean NOT NULL DEFAULT false;
CREATE TABLE take_down_requests (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    number text UNIQUE,
    email_hash text NOT NULL,
    details text NOT NULL,
    status text NOT NULL DEFAULT 'received' CHECK (status IN ('received','under_review','removed','not_removed')),
    received_at timestamptz NOT NULL DEFAULT now(),
    deadline timestamptz NOT NULL DEFAULT now() + interval '48 hours',
    resolved_at timestamptz,
    reason text NOT NULL DEFAULT '',
    last_alert_at timestamptz NOT NULL DEFAULT now(),
    minor boolean NOT NULL DEFAULT false,
    preservation_reference text NOT NULL DEFAULT ''
);
CREATE INDEX take_down_open ON take_down_requests(deadline) WHERE resolved_at IS NULL;
CREATE TABLE take_down_deliveries (
    mail_id text PRIMARY KEY,
    request_id bigint NOT NULL REFERENCES take_down_requests(id) ON DELETE CASCADE,
    audience text NOT NULL,
    queued_at timestamptz NOT NULL DEFAULT now(),
    attempts integer NOT NULL DEFAULT 0,
    sent_at timestamptz
);
CREATE TABLE take_down_events (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    request_id bigint NOT NULL REFERENCES take_down_requests(id) ON DELETE CASCADE,
    at timestamptz NOT NULL DEFAULT now(),
    actor_id text,
    action text NOT NULL,
    detail text NOT NULL DEFAULT ''
);
CREATE TABLE take_down_targets (
    request_id bigint NOT NULL REFERENCES take_down_requests(id) ON DELETE CASCADE,
    kind text NOT NULL,
    target_id text NOT NULL,
    owner_id text NOT NULL,
    snapshot text NOT NULL,
    PRIMARY KEY (request_id,kind,target_id)
);
-- Fingerprints include source bytes, decoded pixels and each published variant. They survive
-- deletion so a known removed image cannot be uploaded again under another kind or crop.
CREATE TABLE media_fingerprints (
    root text PRIMARY KEY,
    hashes text[] NOT NULL
);
CREATE INDEX media_fingerprints_hashes ON media_fingerprints USING gin(hashes);
CREATE TABLE media_removal_holds (
    root text PRIMARY KEY,
    saved text,
    permanent boolean NOT NULL DEFAULT false,
    legal_hold boolean NOT NULL DEFAULT false,
    hidden_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE take_down_media (
    request_id bigint NOT NULL REFERENCES take_down_requests(id) ON DELETE CASCADE,
    root text NOT NULL,
    PRIMARY KEY(request_id,root)
);
CREATE TABLE blocked_media_hashes (hash text PRIMARY KEY, blocked_at timestamptz NOT NULL DEFAULT now());
CREATE TABLE staff_push_subscriptions (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    subscription text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TABLE staff_push_jobs (
    id text PRIMARY KEY,
    subscription_id text NOT NULL REFERENCES staff_push_subscriptions(id) ON DELETE CASCADE,
    attempts integer NOT NULL DEFAULT 0,
    available_at timestamptz NOT NULL DEFAULT now(),
    created_at timestamptz NOT NULL DEFAULT now(),
    delivered_at timestamptz
);
