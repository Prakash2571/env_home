use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::Account;
use crate::otp::{STEP_SECONDS, code_at, now_seconds, step_of};

const REPLY_TIMEOUT_SECONDS: u64 = 150;
const MAX_REPLY_BYTES: usize = 64 * 1024;
pub const MIN_CODE_LIFETIME_SECONDS: u64 = 25;
const NO_STEP: u64 = u64::MAX;

#[derive(Serialize)]
struct Request<'a> {
    #[serde(rename = "consentUrl")]
    consent_url: &'a str,
    #[serde(rename = "redirectUrl")]
    redirect_url: &'a str,
    #[serde(rename = "accountId")]
    account_id: &'a str,
    totp: &'a str,
    pin: &'a str,
}

#[derive(Deserialize)]
struct Reply {
    ok: bool,
    #[serde(rename = "tokenId")]
    token_id: Option<String>,
    stage: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct SidecarFailure {
    pub stage: String,
    pub detail: String,
    pub hopeless: bool,
}

impl std::fmt::Display for SidecarFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "the browser login failed at {}: {}",
            self.stage, self.detail
        )
    }
}

impl std::error::Error for SidecarFailure {}

pub fn hopeless(stage: &str, detail: &str) -> bool {
    let lower = format!("{stage} {detail}").to_lowercase();
    let asked_for_sms = stage == "otp";
    let locked = lower.contains("-rejected")
        && ["too many", "locked", "blocked", "try again later"]
            .iter()
            .any(|phrase| lower.contains(phrase));
    asked_for_sms || locked
}

pub fn wait_for_fresh_code(last_step: Option<u64>, now: u64, min_lifetime: u64) -> u64 {
    let remaining = STEP_SECONDS - (now % STEP_SECONDS);
    let reused = last_step.is_some_and(|last| step_of(now) <= last);
    if reused || remaining < min_lifetime {
        remaining + 1
    } else {
        0
    }
}

#[derive(Clone)]
pub struct Sidecar {
    endpoint: String,
    client: Client,
    last_step: Arc<AtomicU64>,
    min_code_lifetime: u64,
}

impl Sidecar {
    pub fn new(endpoint: &str) -> Result<Self> {
        Self::with_code_lifetime(endpoint, MIN_CODE_LIFETIME_SECONDS)
    }

    pub fn with_code_lifetime(endpoint: &str, min_code_lifetime: u64) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(REPLY_TIMEOUT_SECONDS))
            .build()
            .map_err(|_| anyhow!("could not initialise the browser sidecar client"))?;
        Ok(Self {
            endpoint: endpoint.to_owned(),
            client,
            last_step: Arc::new(AtomicU64::new(NO_STEP)),
            min_code_lifetime,
        })
    }

    async fn fresh_code(&self, account: &Account) -> String {
        loop {
            let last = Some(self.last_step.load(Ordering::Relaxed)).filter(|step| *step != NO_STEP);
            let wait = wait_for_fresh_code(last, now_seconds(), self.min_code_lifetime);
            if wait == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_secs(wait)).await;
        }
        let now = now_seconds();
        self.last_step.store(step_of(now), Ordering::Relaxed);
        code_at(&account.dhan_totp_seed, now)
    }

    pub async fn drive(
        &self,
        account: &Account,
        consent_url: &str,
        redirect_url: &str,
    ) -> Result<String> {
        let code = self.fresh_code(account).await;
        let response = self
            .client
            .post(&self.endpoint)
            .json(&Request {
                consent_url,
                redirect_url,
                account_id: &account.account_id,
                totp: &code,
                pin: &account.pin,
            })
            .send()
            .await
            .map_err(|_| anyhow!("the browser sidecar did not answer"))?;

        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|_| anyhow!("could not read the browser sidecar reply"))?;
        if body.len() > MAX_REPLY_BYTES {
            bail!("the browser sidecar reply exceeded the size limit");
        }
        let reply: Reply = serde_json::from_slice(&body).map_err(|_| {
            anyhow!(
                "the browser sidecar answered HTTP {} with an unreadable body",
                status.as_u16()
            )
        })?;

        if !reply.ok {
            let stage = reply.stage.unwrap_or_else(|| "unknown".to_owned());
            let detail = reply
                .error
                .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
            let hopeless = hopeless(&stage, &detail);
            return Err(SidecarFailure {
                stage,
                detail,
                hopeless,
            }
            .into());
        }
        match reply.token_id {
            Some(token_id) if !token_id.trim().is_empty() => Ok(token_id),
            _ => bail!("the browser login reported success without a tokenId"),
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/dhan_sidecar.rs"]
mod tests;
