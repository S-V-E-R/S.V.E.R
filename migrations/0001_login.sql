CREATE TABLE users (
    id text PRIMARY KEY,
    email text NOT NULL,
    username text NOT NULL,
    password_hash text,
    date_of_birth date,
    email_verified boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now(),
    deleted_at timestamptz,
    mfa_secret text,
    mfa_enabled boolean NOT NULL DEFAULT false,
    mfa_last_step bigint NOT NULL DEFAULT -1,
    auth_version bigint NOT NULL DEFAULT 0,
    CHECK (NOT mfa_enabled OR mfa_secret IS NOT NULL)
);
CREATE UNIQUE INDEX users_email ON users (lower(email));
CREATE UNIQUE INDEX users_username ON users (lower(username));
CREATE INDEX users_deletion ON users (deleted_at) WHERE deleted_at IS NOT NULL;

CREATE TABLE identities (
    provider text NOT NULL CHECK (provider IN ('google', 'twitch', 'discord')),
    subject text NOT NULL,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    PRIMARY KEY (provider, subject),
    UNIQUE(user_id, provider)
);
CREATE TABLE sessions (
    id text PRIMARY KEY,
    token_hash text UNIQUE NOT NULL,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    auth_version bigint NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    last_seen_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL DEFAULT now() + interval '30 days',
    authenticated_at timestamptz NOT NULL DEFAULT now(),
    mfa_verified boolean NOT NULL DEFAULT false,
    user_agent text NOT NULL
);
CREATE INDEX sessions_user ON sessions(user_id);
CREATE TABLE challenges (
    token_hash text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind text NOT NULL CHECK (kind IN ('verify', 'reset', 'mfa', 'setup')),
    auth_version bigint NOT NULL,
    browser_hash text,
    payload text,
    expires_at timestamptz NOT NULL
);
CREATE INDEX challenges_user ON challenges(user_id, kind);
CREATE TABLE recovery_codes (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash text NOT NULL
);
CREATE INDEX recovery_user ON recovery_codes(user_id);
CREATE TABLE rate_limits (
    key text PRIMARY KEY,
    count integer NOT NULL,
    expires_at timestamptz NOT NULL
);
CREATE TABLE oauth_states (
    state_hash text PRIMARY KEY,
    browser_hash text NOT NULL,
    provider text NOT NULL,
    intent text NOT NULL CHECK(intent IN ('login', 'signup', 'link', 'reauth')),
    session_id text REFERENCES sessions(id) ON DELETE CASCADE,
    payload text NOT NULL,
    expires_at timestamptz NOT NULL DEFAULT now() + interval '10 minutes'
);
CREATE TABLE mail_jobs (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    payload text NOT NULL,
    attempts integer NOT NULL DEFAULT 0,
    available_at timestamptz NOT NULL DEFAULT now(),
    expires_at timestamptz NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX mail_jobs_due ON mail_jobs(available_at);
