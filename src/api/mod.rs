mod accounts;
mod admin;
pub mod guard;
mod public;
mod token;

use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, HeaderValue, header};
use axum::routing::{delete, get, post};
use serde::Serialize;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::clock::now_unix;
use crate::config::{AdminPolicy, ApiPolicy};
use crate::login::{LoginEngine, SchedulerHandle};
use crate::state::{AccountView, EventView, client_id_hint};
use crate::store::{AttemptRecord, RequestRecord};
use crate::telegram::TelegramView;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const BODY_LIMIT: usize = 16 * 1024;
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
     img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

pub struct Running {
    pub engine: Arc<LoginEngine>,
    pub book: crate::accounts::AccountBook,
    pub scheduler: SchedulerHandle,
    pub window_label: String,
    pub window: crate::clock::LoginWindow,
    pub api: ApiPolicy,
    pub admin: AdminPolicy,
    pub callback_route: String,
}

pub struct AppContext {
    pub running: Option<Running>,
    pub admin: AdminPolicy,
    pub problems: Vec<String>,
    pub guard: guard::TokenGuard,
    pub started_at: i64,
}

pub type Context = Arc<AppContext>;

#[derive(Serialize)]
pub struct WindowView {
    pub label: String,
    pub open: bool,
    pub opens_in: i64,
    pub closes_in: i64,
}

#[derive(Serialize)]
pub struct Snapshot {
    pub configured: bool,
    pub problems: Vec<String>,
    pub version: &'static str,
    pub started_at: i64,
    pub generated_at: i64,
    pub store: Option<&'static str>,
    pub store_reachable: bool,
    pub automation: bool,
    pub accounts_loaded: bool,
    pub api_requires_access_jwt: bool,
    pub window: Option<WindowView>,
    pub accounts: Vec<AccountView>,
    pub telegram: TelegramView,
    pub events: Vec<EventView>,
    pub requests: Vec<RequestRecord>,
    pub attempts: Vec<AttemptRecord>,
}

pub async fn snapshot(context: &AppContext) -> Snapshot {
    let now = now_unix();
    let Some(running) = &context.running else {
        return Snapshot {
            configured: false,
            problems: context.problems.clone(),
            version: VERSION,
            started_at: context.started_at,
            generated_at: now,
            store: None,
            store_reachable: false,
            automation: false,
            accounts_loaded: false,
            api_requires_access_jwt: false,
            window: None,
            accounts: Vec::new(),
            telegram: TelegramView::default(),
            events: Vec::new(),
            requests: Vec::new(),
            attempts: Vec::new(),
        };
    };
    let store = running.engine.vault().store();
    Snapshot {
        configured: true,
        problems: Vec::new(),
        version: VERSION,
        started_at: context.started_at,
        generated_at: now,
        store: Some(store.kind()),
        store_reachable: store.is_reachable().await,
        automation: running.engine.automation_available(),
        accounts_loaded: running.engine.registry().is_loaded(),
        api_requires_access_jwt: running.api.require_access_jwt,
        window: Some(WindowView {
            label: running.window_label.clone(),
            open: running.window.is_open(now),
            opens_in: running.window.seconds_until_open(now),
            closes_in: running.window.seconds_left(now),
        }),
        accounts: running.engine.live().accounts(),
        telegram: running.engine.notifier().view(),
        events: running.engine.live().events(),
        requests: store
            .recent_requests(25)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|request| RequestRecord {
                client_id: if request.client_id.starts_with('<') {
                    request.client_id.clone()
                } else {
                    client_id_hint(&request.client_id)
                },
                ..request
            })
            .collect(),
        attempts: store.recent_attempts(25).await.unwrap_or_default(),
    }
}

fn header_layer(name: HeaderName, value: &'static str) -> SetResponseHeaderLayer<HeaderValue> {
    SetResponseHeaderLayer::overriding(name, HeaderValue::from_static(value))
}

pub fn router(context: Context, console: &Path) -> Router {
    let callback_route = context.running.as_ref().map_or_else(
        || "/dhan/callback".to_owned(),
        |running| running.callback_route.clone(),
    );

    let index = console.join("index.html");
    let static_files = ServeDir::new(console).fallback(ServeFile::new(&index));
    if index.is_file() {
        tracing::info!("serving the console from {}", console.display());
    } else {
        tracing::warn!(
            "no built console at {}; only the API is served",
            console.display()
        );
    }

    Router::new()
        .route("/health", get(public::health))
        .route("/robots.txt", get(public::robots))
        .route(&callback_route, get(public::dhan_callback))
        .route("/api/state", get(admin::state))
        .route("/api/stream", get(admin::stream))
        .route("/api/accounts", post(accounts::add))
        .route("/api/accounts/{alias}", delete(accounts::remove))
        .route(
            "/api/accounts/{alias}/token-seed",
            post(accounts::rotate_seed),
        )
        .route("/api/accounts/{alias}/login", post(admin::request_login))
        .route("/api/accounts/login", post(admin::request_login_all))
        .route("/api/telegram/test", post(admin::test_telegram))
        .route("/api/v1/token", post(token::issue))
        .fallback_service(static_files)
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(header_layer(
            HeaderName::from_static("x-robots-tag"),
            "noindex, nofollow, noarchive, nosnippet",
        ))
        .layer(header_layer(header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
        .layer(header_layer(header::X_FRAME_OPTIONS, "DENY"))
        .layer(header_layer(header::REFERRER_POLICY, "no-referrer"))
        .layer(header_layer(
            header::CONTENT_SECURITY_POLICY,
            CONTENT_SECURITY_POLICY,
        ))
        .with_state(context)
}

pub async fn serve(context: Context, console: &Path, address: SocketAddr) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!("listening on {address}");
    axum::serve(
        listener,
        router(context, console).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/api.rs"]
mod tests;

#[cfg(test)]
#[path = "../../tests/unit/api_accounts.rs"]
mod account_tests;
