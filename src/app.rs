use std::sync::Arc;

use anyhow::{Context as _, Result};

use crate::api::{self, AppContext, Running, VERSION};
use crate::clock::now_unix;
use crate::config::{self, AdminPolicy, Config, StoreConfig};
use crate::crypto::Cipher;
use crate::dhan::api::DhanAuth;
use crate::dhan::callback::{CallbackGate, route_of};
use crate::dhan::sidecar::Sidecar;
use crate::login::{EngineParts, LoginEngine, MANUAL_TIMEOUT, Scheduler};
use crate::state::{EventKind, LiveState};
use crate::store::{MemoryStore, PgStore, Store};
use crate::telegram::{Channel, Notice, Notifier, Topic};
use crate::vault::Vault;

pub fn unconfigured(problems: Vec<String>, started_at: i64, admin: AdminPolicy) -> AppContext {
    AppContext {
        running: None,
        admin,
        problems,
        guard: api::guard::TokenGuard::default(),
        started_at,
    }
}

pub async fn build(config: Config, started_at: i64) -> Result<AppContext> {
    let store = match &config.store {
        StoreConfig::Memory => Store::Memory(MemoryStore::default()),
        StoreConfig::Postgres(params) => Store::Postgres(PgStore::new(params)),
    };
    if !store.is_reachable().await {
        tracing::warn!(
            "the {} store is not reachable yet; it is retried on every use",
            store.kind()
        );
    }
    let cipher = Cipher::from_hex(&config.encryption_key).context("CBL_ENCRYPTION_KEY")?;
    let notifier = match &config.telegram {
        Some(telegram) => Notifier::start(Channel::new(telegram.clone())?),
        None => Notifier::disabled(),
    };
    let sidecar = config
        .sidecar_url
        .as_deref()
        .map(Sidecar::new)
        .transpose()?;
    let live = LiveState::new(&config.accounts);
    let engine = LoginEngine::new(EngineParts {
        accounts: config.accounts.clone(),
        dhan: DhanAuth::new(&config.dhan_auth_base)?,
        sidecar,
        gate: CallbackGate::new(config.redirect_url.clone()),
        vault: Vault::new(store, cipher),
        live,
        notifier: notifier.clone(),
        manual_timeout: MANUAL_TIMEOUT,
    });
    engine.refresh_token_views().await;

    let (scheduler, handle) = Scheduler::new(
        Arc::clone(&engine),
        config.window.clone(),
        Arc::new(now_unix),
    );
    tokio::spawn(scheduler.run());

    let aliases: Vec<&str> = config
        .accounts
        .iter()
        .map(|account| account.alias.as_str())
        .collect();
    engine.live().note(
        EventKind::System,
        None,
        format!("service started with {}", aliases.join(", ")),
    );
    notifier.send(
        &Notice::info(Topic::System, "the broker login service started")
            .fact("version", VERSION)
            .fact("accounts", aliases.join(", "))
            .fact("login window", config.window.label())
            .fact(
                "browser sidecar",
                if engine.automation_available() {
                    "configured"
                } else {
                    "not set"
                },
            ),
    );
    tracing::info!(accounts = %aliases.join(","), window = %config.window.label(), "ready");

    Ok(AppContext {
        running: Some(Running {
            engine,
            scheduler: handle,
            window_label: config.window.label(),
            window: config.window.clone(),
            api: config.api.clone(),
            admin: config.admin.clone(),
            callback_route: route_of(&config.redirect_url),
        }),
        admin: config.admin.clone(),
        problems: Vec::new(),
        guard: api::guard::TokenGuard::default(),
        started_at,
    })
}

pub async fn run() -> Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .init();

    let read = |name: &str| std::env::var(name).ok();
    let basics = config::basics(&read);
    let console = basics.home.join("ui").join("dist");
    let started_at = now_unix();
    tracing::info!(version = VERSION, "starting the broker login service");

    let context = match config::load(&read) {
        Ok(config) => build(config, started_at).await?,
        Err(gap) => {
            tracing::error!(
                "configuration incomplete: {}; serving /health and the dashboard only",
                gap.problems.join("; ")
            );
            unconfigured(gap.problems, started_at, basics.admin.clone())
        }
    };
    api::serve(Arc::new(context), &console, basics.http_addr).await
}

#[cfg(test)]
#[path = "../tests/unit/app.rs"]
mod tests;
