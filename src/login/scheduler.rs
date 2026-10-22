use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;

use super::{Drive, LoginEngine};
use crate::clock::{LoginWindow, describe, ist_day_number};
use crate::state::EventKind;
use crate::telegram::{Notice, Topic};

pub const MAX_WINDOW_ATTEMPTS: u32 = 5;
pub const RETRY_GAP: Duration = Duration::from_secs(60);
const REQUEST_QUEUE: usize = 16;

pub type Now = Arc<dyn Fn() -> i64 + Send + Sync>;

#[derive(Clone, Debug)]
pub struct LoginRequest {
    pub alias: Option<String>,
    pub drive: Drive,
}

#[derive(Clone)]
pub struct SchedulerHandle {
    sender: mpsc::Sender<LoginRequest>,
}

impl SchedulerHandle {
    pub fn request(&self, request: LoginRequest) -> bool {
        self.sender.try_send(request).is_ok()
    }
}

#[derive(Default)]
struct DayLedger {
    day: i64,
    attempts: HashMap<String, u32>,
    stopped: HashSet<String>,
    summary_sent: bool,
}

pub struct Scheduler {
    engine: Arc<LoginEngine>,
    window: LoginWindow,
    receiver: mpsc::Receiver<LoginRequest>,
    now: Now,
    retry_gap: Duration,
    ledger: DayLedger,
}

impl Scheduler {
    pub fn new(engine: Arc<LoginEngine>, window: LoginWindow, now: Now) -> (Self, SchedulerHandle) {
        let (sender, receiver) = mpsc::channel(REQUEST_QUEUE);
        let scheduler = Self {
            engine,
            window,
            receiver,
            now,
            retry_gap: RETRY_GAP,
            ledger: DayLedger::default(),
        };
        (scheduler, SchedulerHandle { sender })
    }

    pub fn window(&self) -> &LoginWindow {
        &self.window
    }

    pub async fn run(mut self) {
        loop {
            let wait = self.tick().await;
            if !self.wait_or_serve(wait).await {
                return;
            }
        }
    }

    fn roll_day(&mut self, now: i64) {
        let today = ist_day_number(now);
        if self.ledger.day != today {
            self.ledger = DayLedger {
                day: today,
                ..DayLedger::default()
            };
        }
    }

    async fn due(&self) -> Vec<String> {
        let mut due = Vec::new();
        for account in self.engine.accounts() {
            let spent = self
                .ledger
                .attempts
                .get(&account.alias)
                .copied()
                .unwrap_or(0);
            if spent >= MAX_WINDOW_ATTEMPTS || self.ledger.stopped.contains(&account.alias) {
                continue;
            }
            match self.engine.vault().obtained_today(account).await {
                Ok(true) => {}
                Ok(false) => due.push(account.alias.clone()),
                Err(error) => {
                    tracing::warn!(alias = %account.alias, %error, "could not check today's token")
                }
            }
        }
        due
    }

    pub async fn tick(&mut self) -> Duration {
        let now = (self.now)();
        self.roll_day(now);
        if self.window.is_open(now) {
            let due = self.due().await;
            if due.is_empty() {
                return Duration::from_secs(self.window.seconds_left(now).max(1) as u64);
            }
            for alias in due {
                *self.ledger.attempts.entry(alias.clone()).or_default() += 1;
                if let Err(failure) = self.engine.login(&alias, Drive::Scheduled).await
                    && failure.hopeless
                {
                    self.ledger.stopped.insert(alias);
                }
            }
            let left = self.window.seconds_left((self.now)()).max(1) as u64;
            return self.retry_gap.min(Duration::from_secs(left));
        }
        if self.window.has_passed_today(now) && !self.ledger.summary_sent {
            self.ledger.summary_sent = true;
            self.send_summary().await;
        }
        Duration::from_secs(self.window.seconds_until_open(now).max(1) as u64)
    }

    async fn send_summary(&self) {
        let mut ready = Vec::new();
        let mut missing = Vec::new();
        for account in self.engine.accounts() {
            if self
                .engine
                .vault()
                .obtained_today(account)
                .await
                .unwrap_or(false)
            {
                ready.push(account.alias.clone());
            } else {
                missing.push(account.alias.clone());
            }
        }
        let summary = if missing.is_empty() {
            Notice::info(Topic::Login, "every account logged in this morning")
        } else {
            Notice::problem(Topic::Login, "some accounts have no token today")
                .next("trading engines asking for these accounts get 503 until a login runs; use the dashboard")
        };
        let joined = |list: &[String]| {
            if list.is_empty() {
                "none".to_owned()
            } else {
                list.join(", ")
            }
        };
        self.engine.notifier().send(
            &summary
                .fact("window", self.window.label())
                .fact("ready", joined(&ready))
                .fact("missing", joined(&missing)),
        );
        self.engine.live().note(
            if missing.is_empty() {
                EventKind::Login
            } else {
                EventKind::Problem
            },
            None,
            format!(
                "morning summary: ready {}, missing {}",
                joined(&ready),
                joined(&missing)
            ),
        );
    }

    async fn wait_or_serve(&mut self, wait: Duration) -> bool {
        tracing::debug!(wait = %describe(wait.as_secs() as i64), "scheduler sleeping");
        tokio::select! {
            () = tokio::time::sleep(wait) => true,
            request = self.receiver.recv() => match request {
                Some(request) => {
                    self.serve(request);
                    true
                }
                None => false,
            },
        }
    }

    fn serve(&self, request: LoginRequest) {
        let aliases: Vec<String> = match &request.alias {
            Some(alias) => vec![alias.clone()],
            None => self
                .engine
                .accounts()
                .iter()
                .map(|account| account.alias.clone())
                .collect(),
        };
        let engine = Arc::clone(&self.engine);
        tokio::spawn(async move {
            for alias in aliases {
                let _ = engine.login(&alias, request.drive).await;
            }
        });
    }
}
