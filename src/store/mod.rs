mod memory;
mod postgres;

use anyhow::Result;
use serde::Serialize;

pub use memory::MemoryStore;
pub use postgres::PgStore;

pub const KEEP_HISTORY_SECONDS: i64 = 90 * 86_400;
pub const KEEP_TOKENS_SECONDS: i64 = 30 * 86_400;

pub const MIGRATIONS: [(i32, &str); 2] = [
    (1, include_str!("../../migrations/0001_init.sql")),
    (2, include_str!("../../migrations/0002_accounts.sql")),
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredToken {
    pub alias: String,
    pub client_id: String,
    pub sealed: String,
    pub obtained_at: i64,
    pub expires_at: i64,
    pub drive: String,
}

/// An account's sealed secrets. The store lists them in the order they were added.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredAccount {
    pub alias: String,
    pub client_id: String,
    pub sealed: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Why an insert was refused: the row already there is left exactly as it was.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccountConflict {
    Alias,
    ClientId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AttemptRecord {
    pub alias: String,
    pub drive: String,
    pub started_at: i64,
    pub finished_at: i64,
    pub outcome: String,
    pub stage: String,
    pub detail: String,
    pub consent_spent: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RequestRecord {
    pub at: i64,
    pub client_id: String,
    pub alias: Option<String>,
    pub ip: Option<String>,
    pub outcome: String,
    pub reason: String,
}

#[derive(Clone)]
pub enum Store {
    Memory(MemoryStore),
    Postgres(PgStore),
}

impl Store {
    pub async fn save_token(&self, token: &StoredToken) -> Result<()> {
        match self {
            Self::Memory(store) => store.save_token(token),
            Self::Postgres(store) => store.save_token(token).await,
        }
    }

    pub async fn latest_token(&self, alias: &str) -> Result<Option<StoredToken>> {
        match self {
            Self::Memory(store) => Ok(store.latest_token(alias)),
            Self::Postgres(store) => store.latest_token(alias).await,
        }
    }

    pub async fn list_accounts(&self) -> Result<Vec<StoredAccount>> {
        match self {
            Self::Memory(store) => Ok(store.list_accounts()),
            Self::Postgres(store) => store.list_accounts().await,
        }
    }

    /// Adds a new account. Never overwrites: a taken alias or client ID comes
    /// back as a conflict, even for a row the service could not open.
    pub async fn insert_account(&self, account: &StoredAccount) -> Result<Option<AccountConflict>> {
        match self {
            Self::Memory(store) => Ok(store.insert_account(account)),
            Self::Postgres(store) => store.insert_account(account).await,
        }
    }

    /// Replaces an account's sealed secrets, only where both the alias and the
    /// client ID match, keeping `created_at`. False when no such row exists.
    pub async fn reseal_account(
        &self,
        alias: &str,
        client_id: &str,
        sealed: &str,
        at: i64,
    ) -> Result<bool> {
        match self {
            Self::Memory(store) => Ok(store.reseal_account(alias, client_id, sealed, at)),
            Self::Postgres(store) => store.reseal_account(alias, client_id, sealed, at).await,
        }
    }

    /// Removes the account and every token stored for it; its history stays.
    pub async fn delete_account(&self, alias: &str) -> Result<bool> {
        match self {
            Self::Memory(store) => Ok(store.delete_account(alias)),
            Self::Postgres(store) => store.delete_account(alias).await,
        }
    }

    pub async fn record_attempt(&self, attempt: &AttemptRecord) -> Result<()> {
        match self {
            Self::Memory(store) => store.record_attempt(attempt),
            Self::Postgres(store) => store.record_attempt(attempt).await,
        }
    }

    pub async fn recent_attempts(&self, limit: i64) -> Result<Vec<AttemptRecord>> {
        match self {
            Self::Memory(store) => Ok(store.recent_attempts(limit)),
            Self::Postgres(store) => store.recent_attempts(limit).await,
        }
    }

    pub async fn consents_since(&self, alias: &str, since: i64) -> Result<u32> {
        match self {
            Self::Memory(store) => Ok(store.consents_since(alias, since)),
            Self::Postgres(store) => store.consents_since(alias, since).await,
        }
    }

    pub async fn record_request(&self, request: &RequestRecord) -> Result<()> {
        match self {
            Self::Memory(store) => store.record_request(request),
            Self::Postgres(store) => store.record_request(request).await,
        }
    }

    pub async fn recent_requests(&self, limit: i64) -> Result<Vec<RequestRecord>> {
        match self {
            Self::Memory(store) => Ok(store.recent_requests(limit)),
            Self::Postgres(store) => store.recent_requests(limit).await,
        }
    }

    pub async fn prune(&self, now: i64) -> Result<u64> {
        let history = now - KEEP_HISTORY_SECONDS;
        let tokens = now - KEEP_TOKENS_SECONDS;
        match self {
            Self::Memory(store) => Ok(store.prune(history, tokens)),
            Self::Postgres(store) => store.prune(history, tokens).await,
        }
    }

    pub async fn is_reachable(&self) -> bool {
        match self {
            Self::Memory(_) => true,
            Self::Postgres(store) => store.is_reachable().await,
        }
    }

    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Memory(_) => "memory",
            Self::Postgres(_) => "postgres",
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/store.rs"]
mod tests;
