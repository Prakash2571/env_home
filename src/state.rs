use std::collections::VecDeque;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use serde::Serialize;
use tokio::sync::watch;

use crate::accounts::Account;
use crate::clock::now_unix;

const MAX_EVENTS: usize = 200;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StageId {
    Consent,
    Browser,
    Callback,
    Exchange,
    Stored,
    Notified,
}

pub const STAGES: [StageId; 6] = [
    StageId::Consent,
    StageId::Browser,
    StageId::Callback,
    StageId::Exchange,
    StageId::Stored,
    StageId::Notified,
];

impl StageId {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Consent => "consent",
            Self::Browser => "browser: account, PIN, TOTP",
            Self::Callback => "callback",
            Self::Exchange => "token exchange",
            Self::Stored => "stored encrypted",
            Self::Notified => "notified",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    Idle,
    Running,
    Done,
    Failed,
    Skipped,
}

#[derive(Clone, Debug, Serialize)]
pub struct StageView {
    pub id: StageId,
    pub label: &'static str,
    pub status: StageStatus,
    pub detail: String,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    LoggingIn,
    AwaitingCallback,
    Ready,
    Failed,
    StoppedForToday,
}

#[derive(Clone, Debug, Serialize)]
pub struct TokenView {
    pub obtained_at: i64,
    pub expires_at: i64,
    pub drive: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AccountView {
    pub alias: String,
    pub client_id_hint: String,
    pub phase: Phase,
    pub stages: Vec<StageView>,
    pub last_error: Option<String>,
    pub consent_url: Option<String>,
    pub token: Option<TokenView>,
    pub last_served_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Login,
    Problem,
    Api,
    System,
}

#[derive(Clone, Debug, Serialize)]
pub struct EventView {
    pub at: i64,
    pub kind: EventKind,
    pub alias: Option<String>,
    pub message: String,
}

pub fn client_id_hint(client_id: &str) -> String {
    let tail: String = client_id
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("••••{tail}")
}

fn idle_stages() -> Vec<StageView> {
    STAGES
        .iter()
        .map(|id| StageView {
            id: *id,
            label: id.label(),
            status: StageStatus::Idle,
            detail: String::new(),
            started_at: None,
            finished_at: None,
        })
        .collect()
}

fn fresh_view(account: &Account) -> AccountView {
    AccountView {
        alias: account.alias.clone(),
        client_id_hint: client_id_hint(&account.client_id),
        phase: Phase::Idle,
        stages: idle_stages(),
        last_error: None,
        consent_url: None,
        token: None,
        last_served_at: None,
    }
}

struct Inner {
    accounts: Vec<AccountView>,
    events: VecDeque<EventView>,
}

#[derive(Clone)]
pub struct LiveState {
    inner: Arc<RwLock<Inner>>,
    revision: watch::Sender<u64>,
}

impl LiveState {
    pub fn new(accounts: &[Account]) -> Self {
        let views = accounts.iter().map(fresh_view).collect();
        let (revision, _) = watch::channel(0);
        Self {
            inner: Arc::new(RwLock::new(Inner {
                accounts: views,
                events: VecDeque::new(),
            })),
            revision,
        }
    }

    fn read(&self) -> RwLockReadGuard<'_, Inner> {
        self.inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, Inner> {
        self.inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn changed(&self) {
        self.revision.send_modify(|revision| *revision += 1);
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.revision.subscribe()
    }

    pub fn accounts(&self) -> Vec<AccountView> {
        self.read().accounts.clone()
    }

    pub fn account(&self, alias: &str) -> Option<AccountView> {
        self.read()
            .accounts
            .iter()
            .find(|view| view.alias == alias)
            .cloned()
    }

    /// Shows a newly added (or newly loaded) account; one already shown stays.
    pub fn add_account(&self, account: &Account) {
        {
            let mut inner = self.write();
            if inner
                .accounts
                .iter()
                .any(|view| view.alias == account.alias)
            {
                return;
            }
            inner.accounts.push(fresh_view(account));
        }
        self.changed();
    }

    pub fn remove_account(&self, alias: &str) {
        self.write().accounts.retain(|view| view.alias != alias);
        self.changed();
    }

    pub fn events(&self) -> Vec<EventView> {
        self.read().events.iter().rev().cloned().collect()
    }

    pub fn update(&self, alias: &str, change: impl FnOnce(&mut AccountView)) {
        {
            let mut inner = self.write();
            if let Some(view) = inner.accounts.iter_mut().find(|view| view.alias == alias) {
                change(view);
            }
        }
        self.changed();
    }

    pub fn begin_attempt(&self, alias: &str) {
        self.update(alias, |view| {
            view.phase = Phase::LoggingIn;
            view.stages = idle_stages();
            view.last_error = None;
            view.consent_url = None;
        });
    }

    pub fn stage(&self, alias: &str, id: StageId, status: StageStatus, detail: impl Into<String>) {
        let detail = detail.into();
        let now = now_unix();
        self.update(alias, |view| {
            if let Some(stage) = view.stages.iter_mut().find(|stage| stage.id == id) {
                if status == StageStatus::Running {
                    stage.started_at = Some(now);
                    stage.finished_at = None;
                } else if status != StageStatus::Idle {
                    stage.finished_at = Some(now);
                    stage.started_at.get_or_insert(now);
                }
                stage.status = status;
                stage.detail = detail;
            }
        });
    }

    pub fn note(&self, kind: EventKind, alias: Option<&str>, message: impl Into<String>) {
        {
            let mut inner = self.write();
            inner.events.push_back(EventView {
                at: now_unix(),
                kind,
                alias: alias.map(str::to_owned),
                message: message.into(),
            });
            while inner.events.len() > MAX_EVENTS {
                inner.events.pop_front();
            }
        }
        self.changed();
    }
}

#[cfg(test)]
#[path = "../tests/unit/state.rs"]
mod tests;
