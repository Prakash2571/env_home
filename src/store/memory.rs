use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Result;

use super::{AccountConflict, AttemptRecord, RequestRecord, StoredAccount, StoredToken};

const MAX_ROWS: usize = 5_000;

#[derive(Default)]
struct Tables {
    accounts: Vec<StoredAccount>,
    tokens: Vec<StoredToken>,
    attempts: Vec<AttemptRecord>,
    requests: Vec<RequestRecord>,
}

#[derive(Clone, Default)]
pub struct MemoryStore {
    tables: Arc<Mutex<Tables>>,
}

fn bounded<T>(rows: &mut Vec<T>, row: T) {
    rows.push(row);
    if rows.len() > MAX_ROWS {
        rows.drain(..rows.len() - MAX_ROWS);
    }
}

fn newest<T: Clone>(rows: &[T], limit: i64) -> Vec<T> {
    let limit = usize::try_from(limit.max(0)).unwrap_or(0);
    rows.iter().rev().take(limit).cloned().collect()
}

impl MemoryStore {
    fn tables(&self) -> MutexGuard<'_, Tables> {
        self.tables
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn save_token(&self, token: &StoredToken) -> Result<()> {
        bounded(&mut self.tables().tokens, token.clone());
        Ok(())
    }

    pub fn latest_token(&self, alias: &str) -> Option<StoredToken> {
        self.tables()
            .tokens
            .iter()
            .filter(|token| token.alias == alias)
            .max_by_key(|token| token.obtained_at)
            .cloned()
    }

    /// In the order they were added, like the Postgres `id`.
    pub fn list_accounts(&self) -> Vec<StoredAccount> {
        self.tables().accounts.clone()
    }

    pub fn insert_account(&self, account: &StoredAccount) -> Option<AccountConflict> {
        let mut tables = self.tables();
        if tables.accounts.iter().any(|row| row.alias == account.alias) {
            return Some(AccountConflict::Alias);
        }
        if tables
            .accounts
            .iter()
            .any(|row| row.client_id == account.client_id)
        {
            return Some(AccountConflict::ClientId);
        }
        tables.accounts.push(account.clone());
        None
    }

    pub fn reseal_account(&self, alias: &str, client_id: &str, sealed: &str, at: i64) -> bool {
        let mut tables = self.tables();
        let Some(row) = tables
            .accounts
            .iter_mut()
            .find(|row| row.alias == alias && row.client_id == client_id)
        else {
            return false;
        };
        sealed.clone_into(&mut row.sealed);
        row.updated_at = at;
        true
    }

    pub fn delete_account(&self, alias: &str) -> bool {
        let mut tables = self.tables();
        let before = tables.accounts.len();
        tables.accounts.retain(|account| account.alias != alias);
        tables.tokens.retain(|token| token.alias != alias);
        tables.accounts.len() != before
    }

    pub fn record_attempt(&self, attempt: &AttemptRecord) -> Result<()> {
        bounded(&mut self.tables().attempts, attempt.clone());
        Ok(())
    }

    pub fn recent_attempts(&self, limit: i64) -> Vec<AttemptRecord> {
        newest(&self.tables().attempts, limit)
    }

    pub fn consents_since(&self, alias: &str, since: i64) -> u32 {
        let count = self
            .tables()
            .attempts
            .iter()
            .filter(|attempt| {
                attempt.alias == alias && attempt.consent_spent && attempt.started_at >= since
            })
            .count();
        u32::try_from(count).unwrap_or(u32::MAX)
    }

    pub fn prune(&self, history_before: i64, tokens_before: i64) -> u64 {
        let mut tables = self.tables();
        let before = tables.tokens.len() + tables.attempts.len() + tables.requests.len();
        tables
            .tokens
            .retain(|token| token.obtained_at >= tokens_before);
        tables
            .attempts
            .retain(|attempt| attempt.started_at >= history_before);
        tables
            .requests
            .retain(|request| request.at >= history_before);
        let after = tables.tokens.len() + tables.attempts.len() + tables.requests.len();
        u64::try_from(before - after).unwrap_or(0)
    }

    pub fn record_request(&self, request: &RequestRecord) -> Result<()> {
        bounded(&mut self.tables().requests, request.clone());
        Ok(())
    }

    pub fn recent_requests(&self, limit: i64) -> Vec<RequestRecord> {
        newest(&self.tables().requests, limit)
    }
}
