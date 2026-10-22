use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Mutex, MutexGuard};

use aes_gcm::aead::OsRng;
use aes_gcm::aead::rand_core::RngCore;

use crate::crypto::constant_time_eq;
use crate::otp::{code_for_step, is_six_digits, step_of};

pub const FAILURE_LIMIT: usize = 5;
pub const CLIENT_FAILURE_LIMIT: usize = 30;
pub const FAILURE_WINDOW_SECONDS: u64 = 600;
pub const LOCKOUT_SECONDS: u64 = 600;
pub const REQUESTS_PER_MINUTE_PER_IP: usize = 30;
pub const ALERTS_PER_HOUR: usize = 10;
const DRIFT_STEPS: u64 = 1;
const MAX_TRACKED: usize = 10_000;
const ALERT_GAP_SECONDS: u64 = 3_600;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    Malformed,
    Locked { until: u64 },
    Wrong { locked_until: Option<u64> },
    Reused,
}

pub struct Attempt<'request> {
    pub client_id: &'request str,
    pub source: &'request str,
    pub secret: Option<&'request [u8]>,
    pub shared_seed: bool,
    pub code: &'request str,
    pub now: u64,
}

#[derive(Clone, Debug, Default)]
struct Strikes {
    failures: VecDeque<u64>,
    locked_until: Option<u64>,
}

impl Strikes {
    fn locked(&mut self, now: u64) -> Option<u64> {
        match self.locked_until {
            Some(until) if now < until => Some(until),
            Some(_) => {
                self.locked_until = None;
                self.failures.clear();
                None
            }
            None => None,
        }
    }

    fn record(&mut self, now: u64, limit: usize) -> Option<u64> {
        while self
            .failures
            .front()
            .is_some_and(|at| now.saturating_sub(*at) >= FAILURE_WINDOW_SECONDS)
        {
            self.failures.pop_front();
        }
        self.failures.push_back(now);
        if self.failures.len() >= limit {
            let until = now + LOCKOUT_SECONDS;
            self.locked_until = Some(until);
            return Some(until);
        }
        None
    }

    fn is_idle(&self, now: u64) -> bool {
        self.locked_until.is_none_or(|until| now >= until)
            && self
                .failures
                .back()
                .is_none_or(|at| now.saturating_sub(*at) >= FAILURE_WINDOW_SECONDS)
    }
}

fn matching_step(secret: &[u8], code: &str, now: u64) -> Option<u64> {
    let current = step_of(now);
    let first = current.saturating_sub(DRIFT_STEPS);
    let mut found = None;
    for step in first..=current + DRIFT_STEPS {
        if constant_time_eq(&code_for_step(secret, step), code) {
            found = Some(step);
        }
    }
    found
}

fn room<K: std::hash::Hash + Eq + Clone>(map: &mut HashMap<K, Strikes>, key: &K, now: u64) -> bool {
    if map.contains_key(key) || map.len() < MAX_TRACKED {
        return true;
    }
    map.retain(|_, strikes| !strikes.is_idle(now));
    map.len() < MAX_TRACKED
}

pub fn source_key(ip: &str) -> String {
    match ip.parse::<IpAddr>() {
        Ok(IpAddr::V6(address)) => {
            let segments = address.segments();
            format!(
                "{:x}:{:x}:{:x}:{:x}::/64",
                segments[0], segments[1], segments[2], segments[3]
            )
        }
        Ok(IpAddr::V4(address)) => address.to_string(),
        Err(_) => ip.to_owned(),
    }
}

#[derive(Default)]
struct Ledger {
    per_source: HashMap<(String, String), Strikes>,
    per_client: HashMap<String, Strikes>,
    accepted: HashMap<String, u64>,
    ips: HashMap<String, VecDeque<u64>>,
    alerted: HashMap<String, u64>,
    alerts: VecDeque<u64>,
}

pub struct TokenGuard {
    decoy: [u8; 20],
    ledger: Mutex<Ledger>,
}

impl Default for TokenGuard {
    fn default() -> Self {
        let mut decoy = [0u8; 20];
        OsRng.fill_bytes(&mut decoy);
        Self {
            decoy,
            ledger: Mutex::new(Ledger::default()),
        }
    }
}

const SHARED_SEED_KEY: &str = "<shared token API seed>";

impl TokenGuard {
    fn ledger(&self) -> MutexGuard<'_, Ledger> {
        self.ledger
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn check_code(&self, attempt: &Attempt<'_>) -> Result<u64, Refusal> {
        if !is_six_digits(attempt.code) {
            return Err(Refusal::Malformed);
        }
        let now = attempt.now;
        let source = (attempt.client_id.to_owned(), source_key(attempt.source));
        let mut budgets = vec![attempt.client_id.to_owned()];
        if attempt.shared_seed {
            budgets.push(SHARED_SEED_KEY.to_owned());
        }

        let mut ledger = self.ledger();
        let ledger = &mut *ledger;
        let full_until = Refusal::Locked {
            until: now + LOCKOUT_SECONDS,
        };
        if !room(&mut ledger.per_source, &source, now)
            || budgets
                .iter()
                .any(|key| !room(&mut ledger.per_client, key, now))
        {
            return Err(full_until);
        }

        let mut locked = ledger
            .per_source
            .entry(source.clone())
            .or_default()
            .locked(now);
        for key in &budgets {
            locked = locked.max(
                ledger
                    .per_client
                    .entry(key.clone())
                    .or_default()
                    .locked(now),
            );
        }
        if let Some(until) = locked {
            return Err(Refusal::Locked { until });
        }

        let matched = matching_step(attempt.secret.unwrap_or(&self.decoy), attempt.code, now)
            .filter(|_| attempt.secret.is_some());
        let Some(step) = matched else {
            let mut locked_until = ledger
                .per_source
                .entry(source)
                .or_default()
                .record(now, FAILURE_LIMIT);
            for key in &budgets {
                locked_until = locked_until.max(
                    ledger
                        .per_client
                        .entry(key.clone())
                        .or_default()
                        .record(now, CLIENT_FAILURE_LIMIT),
                );
            }
            return Err(Refusal::Wrong { locked_until });
        };

        if ledger
            .accepted
            .get(attempt.client_id)
            .is_some_and(|last| step <= *last)
        {
            return Err(Refusal::Reused);
        }
        ledger.accepted.insert(attempt.client_id.to_owned(), step);
        ledger.per_source.remove(&source);
        Ok(step)
    }

    pub fn admit_ip(&self, ip: &str, now: u64) -> bool {
        let key = source_key(ip);
        let mut ledger = self.ledger();
        let ips = &mut ledger.ips;
        if !ips.contains_key(&key) && ips.len() >= MAX_TRACKED {
            ips.retain(|_, seen| seen.back().is_some_and(|at| now.saturating_sub(*at) < 60));
            if ips.len() >= MAX_TRACKED {
                return false;
            }
        }
        let seen = ips.entry(key).or_default();
        while seen.front().is_some_and(|at| now.saturating_sub(*at) >= 60) {
            seen.pop_front();
        }
        if seen.len() >= REQUESTS_PER_MINUTE_PER_IP {
            return false;
        }
        seen.push_back(now);
        true
    }

    pub fn should_alert(&self, key: &str, now: u64) -> bool {
        let mut ledger = self.ledger();
        while ledger
            .alerts
            .front()
            .is_some_and(|at| now.saturating_sub(*at) >= ALERT_GAP_SECONDS)
        {
            ledger.alerts.pop_front();
        }
        if ledger.alerts.len() >= ALERTS_PER_HOUR
            || ledger
                .alerted
                .get(key)
                .is_some_and(|at| now.saturating_sub(*at) < ALERT_GAP_SECONDS)
        {
            return false;
        }
        if ledger.alerted.len() >= MAX_TRACKED {
            ledger
                .alerted
                .retain(|_, at| now.saturating_sub(*at) < ALERT_GAP_SECONDS);
        }
        ledger.alerted.insert(key.to_owned(), now);
        ledger.alerts.push_back(now);
        true
    }
}

#[cfg(test)]
#[path = "../../tests/unit/api_guard.rs"]
mod tests;
