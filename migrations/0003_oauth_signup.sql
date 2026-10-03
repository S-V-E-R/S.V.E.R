-- Verified provider details wait briefly for signup completion; no account/session exists yet.
CREATE TABLE oauth_signups (
    token_hash text PRIMARY KEY,
    provider text NOT NULL CHECK (provider IN ('google', 'twitch', 'discord')),
    payload text NOT NULL,
    expires_at timestamptz NOT NULL DEFAULT now() + interval '10 minutes'
);
