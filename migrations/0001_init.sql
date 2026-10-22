CREATE TABLE IF NOT EXISTS broker_tokens (
    id BIGSERIAL PRIMARY KEY,
    alias TEXT NOT NULL,
    client_id TEXT NOT NULL,
    sealed_token TEXT NOT NULL,
    obtained_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    drive TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS broker_tokens_alias_obtained ON broker_tokens (alias, obtained_at DESC);

CREATE TABLE IF NOT EXISTS login_attempts (
    id BIGSERIAL PRIMARY KEY,
    alias TEXT NOT NULL,
    drive TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ NOT NULL,
    outcome TEXT NOT NULL,
    stage TEXT NOT NULL,
    detail TEXT NOT NULL,
    consent_spent BOOLEAN NOT NULL
);
CREATE INDEX IF NOT EXISTS login_attempts_alias_started ON login_attempts (alias, started_at DESC);

CREATE TABLE IF NOT EXISTS token_requests (
    id BIGSERIAL PRIMARY KEY,
    at TIMESTAMPTZ NOT NULL,
    client_id TEXT NOT NULL,
    alias TEXT,
    ip TEXT,
    outcome TEXT NOT NULL,
    reason TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS token_requests_at ON token_requests (at DESC);
