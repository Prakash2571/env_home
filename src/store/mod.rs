mod memory;
mod postgres;

use anyhow::Result;
use serde::Serialize;

pub use memory::MemoryStore;
pub use postgres::PgStore;

pub const MIGRATIONS: [(i32, &str); 1] = [(1, include_str!("../../migrations/0001_init.sql"))];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredToken {
    pub alias: String,
    pub client_id: String,
    pub sealed: String,
    pub obtained_at: i64,
    pub expires_at: i64,
    pub drive: String,
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
