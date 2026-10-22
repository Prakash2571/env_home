use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use crate::crypto::constant_time_eq;
use crate::otp::{code_for_step, is_six_digits, step_of};

pub const FAILURE_LIMIT: usize = 5;
pub const FAILURE_WINDOW_SECONDS: u64 = 600;
pub const LOCKOUT_SECONDS: u64 = 600;
pub const REQUESTS_PER_MINUTE_PER_IP: usize = 30;
const DRIFT_STEPS: u64 = 1;
const MAX_TRACKED: usize = 10_000;
const DECOY_SEED: &[u8] = b"no account answers to this client id";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Refusal {
    Malformed,
    Locked { until: u64 },
    Wrong { locked_until: Option<u64> },
    Reused,
}

#[derive(Clone, Debug, Default)]
pub struct CodeGuard {
    failures: VecDeque<u64>,
    locked_until: Option<u64>,
    last_accepted_step: Option<u64>,
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

impl CodeGuard {
    pub fn check(&mut self, secret: &[u8], code: &str, now: u64) -> Result<u64, Refusal> {
        if let Some(until) = self.locked_until {
            if now < until {
                return Err(Refusal::Locked { until });
            }
            self.locked_until = None;
            self.failures.clear();
        }
        if !is_six_digits(code) {
            return Err(Refusal::Malformed);
        }
        let Some(step) = matching_step(secret, code, now) else {
            return Err(Refusal::Wrong {
                locked_until: self.record_failure(now),
            });
        };
        if self.last_accepted_step.is_some_and(|last| step <= last) {
            return Err(Refusal::Reused);
        }
        self.last_accepted_step = Some(step);
        self.failures.clear();
        Ok(step)
    }

    fn record_failure(&mut self, now: u64) -> Option<u64> {
        while self
            .failures
            .front()
            .is_some_and(|at| now.saturating_sub(*at) >= FAILURE_WINDOW_SECONDS)
        {
            self.failures.pop_front();
        }
        self.failures.push_back(now);
        if self.failures.len() >= FAILURE_LIMIT {
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

#[derive(Default)]
pub struct TokenGuard {
    codes: Mutex<HashMap<String, CodeGuard>>,
    ips: Mutex<HashMap<String, VecDeque<u64>>>,
}

impl TokenGuard {
    pub fn check_code(
        &self,
        client_id: &str,
        secret: Option<&[u8]>,
        code: &str,
        now: u64,
    ) -> Result<u64, Refusal> {
        let mut codes = self
            .codes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if codes.len() >= MAX_TRACKED && !codes.contains_key(client_id) {
            codes.retain(|_, guard| !guard.is_idle(now) || guard.last_accepted_step.is_some());
        }
        let guard = codes.entry(client_id.to_owned()).or_default();
        let outcome = guard.check(secret.unwrap_or(DECOY_SEED), code, now);
        match (secret, outcome) {
            (None, Ok(_) | Err(Refusal::Reused)) => Err(Refusal::Wrong {
                locked_until: guard.record_failure(now),
            }),
            (_, outcome) => outcome,
        }
    }

    pub fn admit_ip(&self, ip: &str, now: u64) -> bool {
        let mut ips = self
            .ips
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if ips.len() >= MAX_TRACKED && !ips.contains_key(ip) {
            ips.retain(|_, seen| seen.back().is_some_and(|at| now.saturating_sub(*at) < 60));
        }
        let seen = ips.entry(ip.to_owned()).or_default();
        while seen.front().is_some_and(|at| now.saturating_sub(*at) >= 60) {
            seen.pop_front();
        }
        if seen.len() >= REQUESTS_PER_MINUTE_PER_IP {
            return false;
        }
        seen.push_back(now);
        true
    }
}

#[cfg(test)]
#[path = "../../tests/unit/api_guard.rs"]
mod tests;
