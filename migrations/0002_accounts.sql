-- Dhan accounts added on the dashboard. Every secret sits in sealed_secrets,
-- AES-256-GCM under CBL_ENCRYPTION_KEY, bound to alias and client_id.
-- id keeps the order accounts were added in, which is the order they log in.
CREATE TABLE IF NOT EXISTS broker_accounts (
    id BIGSERIAL UNIQUE,
    alias TEXT PRIMARY KEY,
    client_id TEXT NOT NULL UNIQUE,
    sealed_secrets TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
