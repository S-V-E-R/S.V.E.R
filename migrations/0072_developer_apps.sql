-- Developer platform, part 1 (docs/DEVELOPER_PLATFORM.md §1): apps, OAuth 2.1 with PKCE, and the
-- grants people can see and revoke. Every secret and token is stored as a digest only.
CREATE TABLE dev_apps (
    id text PRIMARY KEY,
    owner_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name text NOT NULL CHECK (char_length(name) BETWEEN 1 AND 40),
    redirect_uris text[] NOT NULL CHECK (cardinality(redirect_uris) BETWEEN 1 AND 10),
    secret_hash text,
    suspended_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX dev_apps_owner ON dev_apps (owner_id);
CREATE TABLE oauth_codes (
    code_hash text PRIMARY KEY,
    app_id text NOT NULL REFERENCES dev_apps(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scopes text[] NOT NULL,
    redirect_uri text NOT NULL,
    challenge text NOT NULL,
    expires_at timestamptz NOT NULL
);
-- One grant per person and app; revoking it ends every token issued under it.
CREATE TABLE oauth_grants (
    id text PRIMARY KEY,
    app_id text NOT NULL REFERENCES dev_apps(id) ON DELETE CASCADE,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    scopes text[] NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    last_used_at timestamptz,
    revoked_at timestamptz
);
CREATE UNIQUE INDEX oauth_grants_live ON oauth_grants (app_id, user_id) WHERE revoked_at IS NULL;
CREATE TABLE oauth_tokens (
    token_hash text PRIMARY KEY,
    grant_id text NOT NULL REFERENCES oauth_grants(id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('access', 'refresh')),
    expires_at timestamptz NOT NULL,
    -- A refresh token works once; using a spent one revokes the grant (reuse detection).
    used_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX oauth_tokens_grant ON oauth_tokens (grant_id);
