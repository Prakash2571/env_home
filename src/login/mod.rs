mod scheduler;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde::Deserialize;
use tokio::sync::Mutex;

pub use scheduler::{LoginRequest, Scheduler, SchedulerHandle};

use crate::accounts::{Account, Registry};
use crate::clock::{IST_OFFSET_SECONDS, describe, ist_day_number, now_unix};
use crate::dhan::Session;
use crate::dhan::api::DhanAuth;
use crate::dhan::callback::CallbackGate;
use crate::dhan::sidecar::{Sidecar, SidecarFailure};
use crate::state::{EventKind, LiveState, Phase, StageId, StageStatus};
use crate::store::AttemptRecord;
use crate::telegram::{Notice, Notifier, Topic};
use crate::vault::{Vault, token_view};

pub const CONSENTS_PER_DAY: u32 = 25;
pub const MANUAL_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Drive {
    Scheduled,
    Automated,
    Manual,
}

impl Drive {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Scheduled => "scheduled",
            Self::Automated => "automated",
            Self::Manual => "manual",
        }
    }
}

#[derive(Debug)]
pub struct LoginFailure {
    pub stage: StageId,
    pub detail: String,
    pub hopeless: bool,
}

impl std::fmt::Display for LoginFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.stage.label(), self.detail)
    }
}

pub struct EngineParts {
    pub accounts: Registry,
    pub dhan: DhanAuth,
    pub sidecar: Option<Sidecar>,
    pub gate: CallbackGate,
    pub vault: Vault,
    pub live: LiveState,
    pub notifier: Notifier,
    pub manual_timeout: Duration,
}

pub struct LoginEngine {
    parts: EngineParts,
    turn: Mutex<()>,
}

fn fail(stage: StageId, detail: impl Into<String>) -> LoginFailure {
    LoginFailure {
        stage,
        detail: detail.into(),
        hopeless: false,
    }
}

pub fn ist_midnight(unix: i64) -> i64 {
    ist_day_number(unix) * 86_400 - IST_OFFSET_SECONDS
}

impl LoginEngine {
    pub fn new(parts: EngineParts) -> Arc<Self> {
        Arc::new(Self {
            parts,
            turn: Mutex::new(()),
        })
    }

    pub fn registry(&self) -> &Registry {
        &self.parts.accounts
    }

    pub fn accounts(&self) -> Vec<Account> {
        self.parts.accounts.list()
    }

    pub fn account(&self, alias: &str) -> Option<Account> {
        self.parts.accounts.get(alias)
    }

    pub fn live(&self) -> &LiveState {
        &self.parts.live
    }

    pub fn vault(&self) -> &Vault {
        &self.parts.vault
    }

    pub fn notifier(&self) -> &Notifier {
        &self.parts.notifier
    }

    pub fn gate(&self) -> &CallbackGate {
        &self.parts.gate
    }

    pub fn automation_available(&self) -> bool {
        self.parts.sidecar.is_some()
    }

    pub async fn refresh_token_views(&self) {
        for account in self.parts.accounts.list() {
            match self.parts.vault.latest(&account).await {
                Ok(token) => self.parts.live.update(&account.alias, |view| {
                    view.token = token.as_ref().map(token_view);
                    if token
                        .as_ref()
                        .is_some_and(|token| token.expires_at > now_unix())
                        && view.phase == Phase::Idle
                    {
                        view.phase = Phase::Ready;
                    }
                }),
                Err(error) => {
                    tracing::warn!(alias = %account.alias, %error, "could not read the stored token")
                }
            }
        }
    }

    pub async fn login(&self, alias: &str, drive: Drive) -> Result<Session, LoginFailure> {
        let account = self
            .account(alias)
            .ok_or_else(|| fail(StageId::Consent, "no such account"))?;
        let _turn = self.turn.lock().await;
        let live = &self.parts.live;
        let started_at = now_unix();
        live.begin_attempt(alias);
        live.note(
            EventKind::Login,
            Some(alias),
            format!("{} login started", drive.label()),
        );

        let mut consent_spent = false;
        let outcome = self.attempt(&account, drive, &mut consent_spent).await;
        let finished_at = now_unix();

        let record = |outcome: &str, stage: StageId, detail: String| AttemptRecord {
            alias: alias.to_owned(),
            drive: drive.label().into(),
            started_at,
            finished_at,
            outcome: outcome.into(),
            stage: stage.label().into(),
            detail,
            consent_spent,
        };
        match &outcome {
            Ok(session) => {
                self.save_attempt(record("ok", StageId::Notified, "token stored".into()))
                    .await;
                self.announce_success(&account, drive, session).await;
            }
            Err(failure) => {
                self.save_attempt(record("failed", failure.stage, failure.detail.clone()))
                    .await;
                self.announce_failure(&account, drive, failure);
            }
        }
        outcome
    }

    async fn save_attempt(&self, attempt: AttemptRecord) {
        if let Err(error) = self.parts.vault.store().record_attempt(&attempt).await {
            tracing::warn!(%error, "could not record the login attempt");
        }
    }

    async fn attempt(
        &self,
        account: &Account,
        drive: Drive,
        consent_spent: &mut bool,
    ) -> Result<Session, LoginFailure> {
        let live = &self.parts.live;
        let alias = account.alias.as_str();

        let used = self
            .parts
            .vault
            .store()
            .consents_since(alias, ist_midnight(now_unix()))
            .await
            .map_err(|error| {
                fail(
                    StageId::Consent,
                    format!("could not count today's consents: {error:#}"),
                )
            })?;
        if used >= CONSENTS_PER_DAY {
            live.stage(
                alias,
                StageId::Consent,
                StageStatus::Failed,
                "today's consent cap is spent",
            );
            return Err(LoginFailure {
                stage: StageId::Consent,
                detail: format!("Dhan's cap of {CONSENTS_PER_DAY} consents for today is spent"),
                hopeless: true,
            });
        }

        live.stage(alias, StageId::Consent, StageStatus::Running, "");
        let consent_url = self
            .parts
            .dhan
            .generate_consent(account)
            .await
            .map_err(|error| {
                live.stage(
                    alias,
                    StageId::Consent,
                    StageStatus::Failed,
                    format!("{error:#}"),
                );
                fail(StageId::Consent, format!("{error:#}"))
            })?;
        *consent_spent = true;
        live.stage(
            alias,
            StageId::Consent,
            StageStatus::Done,
            "consent generated",
        );
        let mut ticket = self.parts.gate.open(alias);

        let token_id = if drive == Drive::Manual {
            live.stage(
                alias,
                StageId::Browser,
                StageStatus::Skipped,
                "a person completes the consent page",
            );
            live.update(alias, |view| {
                view.consent_url = Some(consent_url.clone());
                view.phase = Phase::AwaitingCallback;
            });
            None
        } else {
            let Some(sidecar) = &self.parts.sidecar else {
                live.stage(
                    alias,
                    StageId::Browser,
                    StageStatus::Failed,
                    "CBL_SIDECAR_URL is not set",
                );
                return Err(fail(
                    StageId::Browser,
                    "there is no browser sidecar; use a manual login",
                ));
            };
            live.stage(
                alias,
                StageId::Browser,
                StageStatus::Running,
                "the sidecar is filling the consent page",
            );
            match sidecar
                .drive(account, &consent_url, self.parts.gate.redirect().as_str())
                .await
            {
                Ok(token_id) => {
                    live.stage(
                        alias,
                        StageId::Browser,
                        StageStatus::Done,
                        "account, PIN and TOTP accepted",
                    );
                    live.stage(
                        alias,
                        StageId::Callback,
                        StageStatus::Done,
                        "the sidecar caught the tokenId",
                    );
                    Some(token_id)
                }
                Err(error) => {
                    let hopeless = error
                        .downcast_ref::<SidecarFailure>()
                        .is_some_and(|failure| failure.hopeless);
                    live.stage(
                        alias,
                        StageId::Browser,
                        StageStatus::Failed,
                        format!("{error:#}"),
                    );
                    live.update(alias, |view| view.consent_url = Some(consent_url.clone()));
                    return Err(LoginFailure {
                        stage: StageId::Browser,
                        detail: format!("{error:#}"),
                        hopeless,
                    });
                }
            }
        };

        let session = match token_id {
            Some(token_id) => self.exchange(account, &token_id).await?,
            None => self.await_callback(account, &mut ticket.candidates).await?,
        };
        drop(ticket);

        live.stage(alias, StageId::Stored, StageStatus::Running, "");
        let stored = self
            .parts
            .vault
            .seal(account, &session, drive.label())
            .await
            .map_err(|error| {
                live.stage(
                    alias,
                    StageId::Stored,
                    StageStatus::Failed,
                    format!("{error:#}"),
                );
                fail(StageId::Stored, format!("{error:#}"))
            })?;
        live.stage(
            alias,
            StageId::Stored,
            StageStatus::Done,
            "AES-256-GCM, bound to the account",
        );
        live.update(alias, |view| {
            view.token = Some(token_view(&stored));
            view.consent_url = None;
        });
        Ok(session)
    }

    async fn exchange(&self, account: &Account, token_id: &str) -> Result<Session, LoginFailure> {
        let live = &self.parts.live;
        live.stage(&account.alias, StageId::Exchange, StageStatus::Running, "");
        match self.parts.dhan.exchange_token(account, token_id).await {
            Ok(session) => {
                let left = describe(session.expires_at - now_unix());
                live.stage(
                    &account.alias,
                    StageId::Exchange,
                    StageStatus::Done,
                    format!("valid for {left}"),
                );
                Ok(session)
            }
            Err(error) => {
                live.stage(
                    &account.alias,
                    StageId::Exchange,
                    StageStatus::Failed,
                    format!("{error:#}"),
                );
                Err(fail(StageId::Exchange, format!("{error:#}")))
            }
        }
    }

    async fn await_callback(
        &self,
        account: &Account,
        candidates: &mut tokio::sync::mpsc::Receiver<String>,
    ) -> Result<Session, LoginFailure> {
        let live = &self.parts.live;
        let alias = account.alias.as_str();
        live.stage(
            alias,
            StageId::Callback,
            StageStatus::Running,
            "waiting for the consent page to redirect back",
        );
        let deadline = tokio::time::Instant::now() + self.parts.manual_timeout;
        loop {
            let Ok(candidate) = tokio::time::timeout_at(deadline, candidates.recv()).await else {
                live.stage(
                    alias,
                    StageId::Callback,
                    StageStatus::Failed,
                    "nobody completed the consent page in time",
                );
                return Err(fail(StageId::Callback, "the manual login timed out"));
            };
            let Some(token_id) = candidate else {
                return Err(fail(StageId::Callback, "the callback channel closed"));
            };
            live.stage(
                alias,
                StageId::Callback,
                StageStatus::Done,
                "the consent page redirected back",
            );
            match self.exchange(account, &token_id).await {
                Ok(session) => return Ok(session),
                Err(_) => {
                    live.stage(
                        alias,
                        StageId::Callback,
                        StageStatus::Running,
                        "a callback was rejected; still waiting",
                    );
                }
            }
        }
    }

    async fn announce_success(&self, account: &Account, drive: Drive, session: &Session) {
        let live = &self.parts.live;
        let left = describe(session.expires_at - now_unix());
        live.note(
            EventKind::Login,
            Some(&account.alias),
            format!("logged in; token valid for {left}"),
        );
        let notifier = &self.parts.notifier;
        if notifier.view().configured {
            notifier.send(
                &Notice::info(Topic::Login, format!("{} is logged in", account.alias))
                    .fact("path", drive.label())
                    .fact("client", crate::state::client_id_hint(&account.client_id))
                    .fact("token valid for", &left),
            );
            live.stage(
                &account.alias,
                StageId::Notified,
                StageStatus::Done,
                "Telegram notice queued",
            );
        } else {
            live.stage(
                &account.alias,
                StageId::Notified,
                StageStatus::Skipped,
                "Telegram is not configured",
            );
        }
        live.update(&account.alias, |view| view.phase = Phase::Ready);
    }

    fn announce_failure(&self, account: &Account, drive: Drive, failure: &LoginFailure) {
        let live = &self.parts.live;
        live.note(
            EventKind::Problem,
            Some(&account.alias),
            format!("login failed at {failure}"),
        );
        live.update(&account.alias, |view| {
            view.last_error = Some(failure.to_string());
            view.phase = if failure.hopeless {
                Phase::StoppedForToday
            } else {
                Phase::Failed
            };
        });
        let next = if failure.hopeless {
            "another automatic try today cannot pass; open the consent link on the dashboard"
        } else {
            "the scheduler retries inside the window; the dashboard can retry now"
        };
        self.parts.notifier.send(
            &Notice::problem(Topic::Login, format!("{} did not log in", account.alias))
                .fact("path", drive.label())
                .fact("stage", failure.stage.label())
                .error(&failure.detail)
                .next(next),
        );
    }
}

#[cfg(test)]
#[path = "../../tests/unit/login.rs"]
mod tests;
