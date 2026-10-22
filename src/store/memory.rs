use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::Result;

use super::{AttemptRecord, RequestRecord, StoredToken};

const MAX_ROWS: usize = 5_000;

#[derive(Default)]
struct Tables {
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

    pub fn record_request(&self, request: &RequestRecord) -> Result<()> {
        bounded(&mut self.tables().requests, request.clone());
        Ok(())
    }

    pub fn recent_requests(&self, limit: i64) -> Vec<RequestRecord> {
        newest(&self.tables().requests, limit)
    }
}
