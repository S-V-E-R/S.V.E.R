-- Developer platform §2 webhooks (docs/DEVELOPER_PLATFORM.md "Webhooks for the same events"): a
-- person (signed in, or through an app with their token) registers an HTTPS URL for a list of
-- topics. Deliveries are queued in the same statement that writes the event, signed with the
-- hook's secret, retried 8 times, and the hook disables itself after 50 failures in a row.
CREATE TABLE event_hooks (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- Set when registered through an app: the hook lives only as long as that app's grant.
    app_id text REFERENCES dev_apps(id) ON DELETE CASCADE,
    topics text[] NOT NULL CHECK (cardinality(topics) BETWEEN 1 AND 50),
    url text NOT NULL CHECK (char_length(url) BETWEEN 9 AND 500),
    secret text NOT NULL,
    failures integer NOT NULL DEFAULT 0,
    disabled_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX event_hooks_topics ON event_hooks USING gin (topics) WHERE disabled_at IS NULL;
CREATE INDEX event_hooks_user ON event_hooks (user_id);

CREATE TABLE hook_deliveries (
    id bigserial PRIMARY KEY,
    hook_id text NOT NULL REFERENCES event_hooks(id) ON DELETE CASCADE,
    payload jsonb NOT NULL,
    attempts integer NOT NULL DEFAULT 0,
    available_at timestamptz NOT NULL DEFAULT now(),
    delivered_at timestamptz,
    error text,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX hook_deliveries_due ON hook_deliveries (available_at) WHERE delivered_at IS NULL;

ALTER TABLE notifications DROP CONSTRAINT notifications_kind_check;
ALTER TABLE notifications ADD CONSTRAINT notifications_kind_check CHECK(kind IN ('live','guild_application','guild_decision','guild_invite','squad_invite','sub_ending','hook_disabled'));
