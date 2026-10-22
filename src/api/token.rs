use std::net::SocketAddr;

use axum::Extension;
use axum::Json;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};

use super::guard::{Attempt, Refusal};
use super::{Context, Running};
use crate::clock::{describe, ist_iso, now_unix};
use crate::config::Account;
use crate::otp::{is_six_digits, now_seconds, seconds_remaining};
use crate::state::{EventKind, client_id_hint};
use crate::store::RequestRecord;
use crate::telegram::{Notice, Topic};

pub const ACCESS_JWT_HEADER: &str = "cf-access-jwt-assertion";
const MAX_CLIENT_ID: usize = 32;
const NOT_READY_RETRY_SECONDS: i64 = 300;

#[derive(Deserialize)]
pub struct TokenRequest {
    client_id: Option<String>,
    totp: Option<String>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Outcome {
    Served,
    Refused,
    RateLimited,
}

impl Outcome {
    const fn label(self) -> &'static str {
        match self {
            Self::Served => "served",
            Self::Refused | Self::RateLimited => "refused",
        }
    }
}

struct Reply {
    status: StatusCode,
    body: Value,
    retry_after: Option<i64>,
    outcome: Outcome,
    reason: String,
}

impl Reply {
    fn refused(status: StatusCode, error: &str, reason: impl Into<String>) -> Self {
        Self {
            status,
            body: json!({ "error": error }),
            retry_after: None,
            outcome: Outcome::Refused,
            reason: reason.into(),
        }
    }

    #[must_use]
    fn retry_in(mut self, seconds: i64) -> Self {
        self.retry_after = Some(seconds);
        self
    }

    fn into_response(self) -> Response {
        let mut response = (self.status, Json(self.body)).into_response();
        let headers = response.headers_mut();
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
        if let Some(seconds) = self.retry_after
            && let Ok(value) = HeaderValue::from_str(&seconds.max(1).to_string())
        {
            headers.insert(header::RETRY_AFTER, value);
        }
        response
    }
}

fn client_ip(
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
    header_name: Option<&str>,
) -> Option<String> {
    let forwarded = header_name
        .and_then(|name| headers.get(name))
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(',')
                .next()
                .unwrap_or_default()
                .trim()
                .to_owned()
        })
        .filter(|value| value.parse::<std::net::IpAddr>().is_ok());
    forwarded.or_else(|| peer.map(|peer| peer.ip().to_string()))
}

fn valid_client_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CLIENT_ID
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

pub async fn issue(
    State(context): State<Context>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(running) = &context.running else {
        return Reply::refused(StatusCode::SERVICE_UNAVAILABLE, "not_configured", "")
            .into_response();
    };
    let ip = client_ip(
        &headers,
        peer.map(|Extension(ConnectInfo(address))| address),
        running.api.client_ip_header.as_deref(),
    );
    let request: Option<TokenRequest> = serde_json::from_slice(&body).ok();
    let client_id = request
        .as_ref()
        .and_then(|request| request.client_id.clone())
        .unwrap_or_default();
    let account = running
        .engine
        .accounts()
        .iter()
        .find(|account| account.client_id == client_id);

    let reply = decide(
        &context,
        running,
        &headers,
        ip.as_deref(),
        &client_id,
        request.as_ref(),
        account,
    )
    .await;
    if reply.outcome != Outcome::RateLimited {
        record(running, ip.as_deref(), &client_id, account, &reply).await;
    }
    reply.into_response()
}

async fn record(
    running: &Running,
    ip: Option<&str>,
    client_id: &str,
    account: Option<&Account>,
    reply: &Reply,
) {
    let valid = valid_client_id(client_id);
    let row = RequestRecord {
        at: now_unix(),
        client_id: if valid {
            client_id.to_owned()
        } else {
            "<invalid>".into()
        },
        alias: account.map(|account| account.alias.clone()),
        ip: ip.map(str::to_owned),
        outcome: reply.outcome.label().into(),
        reason: reply.reason.clone(),
    };
    if let Err(error) = running.engine.vault().store().record_request(&row).await {
        tracing::warn!(%error, "could not record a token request");
    }
    let live = running.engine.live();
    let shown = if valid {
        client_id_hint(client_id)
    } else {
        "an invalid client".into()
    };
    let alias = account.map(|account| account.alias.as_str());
    if reply.outcome == Outcome::Served {
        if let Some(alias) = alias {
            live.update(alias, |view| view.last_served_at = Some(row.at));
        }
        live.note(
            EventKind::Api,
            alias,
            format!("token served to {shown} from {}", ip.unwrap_or("?")),
        );
    } else {
        live.note(
            EventKind::Api,
            alias,
            format!("token refused for {shown}: {}", reply.reason),
        );
    }
}

fn seconds_until(until: u64, fallback: i64) -> i64 {
    i64::try_from(until.saturating_sub(now_seconds())).unwrap_or(fallback)
}

async fn decide(
    context: &Context,
    running: &Running,
    headers: &HeaderMap,
    ip: Option<&str>,
    client_id: &str,
    request: Option<&TokenRequest>,
    account: Option<&Account>,
) -> Reply {
    let source = ip.unwrap_or("unknown");
    if !context.guard.admit_ip(source, now_seconds()) {
        let mut reply = Reply::refused(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "too many requests from this IP",
        )
        .retry_in(60);
        reply.outcome = Outcome::RateLimited;
        return reply;
    }
    if running.api.require_access_jwt && headers.get(ACCESS_JWT_HEADER).is_none() {
        return Reply::refused(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "no Cloudflare Access header",
        );
    }
    let Some(request) = request else {
        return Reply::refused(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "the body is not {client_id, totp} JSON",
        );
    };
    let code = request.totp.as_deref().unwrap_or_default();
    if !valid_client_id(client_id) || !is_six_digits(code) {
        return Reply::refused(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "client_id or totp is malformed",
        );
    }

    let attempt = Attempt {
        client_id,
        source,
        secret: account.map(|account| account.api_totp_seed.as_slice()),
        shared_seed: account.is_some_and(|account| account.api_seed_shared),
        code,
        now: now_seconds(),
    };
    match context.guard.check_code(&attempt) {
        Ok(_) => {}
        Err(Refusal::Malformed) => {
            return Reply::refused(StatusCode::BAD_REQUEST, "bad_request", "totp is malformed");
        }
        Err(Refusal::Reused) => {
            let wait = i64::try_from(seconds_remaining(now_seconds())).unwrap_or(30);
            return Reply::refused(
                StatusCode::CONFLICT,
                "code_already_used",
                "a code is accepted once; wait for the next one",
            )
            .retry_in(wait);
        }
        Err(Refusal::Locked { until }) => {
            return Reply::refused(
                StatusCode::TOO_MANY_REQUESTS,
                "locked",
                "locked after repeated wrong codes",
            )
            .retry_in(seconds_until(until, 600));
        }
        Err(Refusal::Wrong { locked_until }) => {
            if let Some(until) = locked_until {
                alert_lockout(context, running, client_id, account, source, until);
            }
            return Reply::refused(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "wrong code or unknown client",
            );
        }
    }

    let account = account.expect("a passing code implies a known account");
    serve(running, account).await
}

fn alert_lockout(
    context: &Context,
    running: &Running,
    client_id: &str,
    account: Option<&Account>,
    source: &str,
    until: u64,
) {
    if !context.guard.should_alert(client_id, now_seconds()) {
        return;
    }
    running.engine.notifier().send(
        &Notice::problem(Topic::Api, "a client was locked out of the token API")
            .fact("client", client_id_hint(client_id))
            .fact("known account", account.map_or("no", |_| "yes"))
            .fact("from", source)
            .fact("locked for", describe(seconds_until(until, 0)))
            .next("if this was not one of your engines, rotate that account's API seed"),
    );
}

async fn serve(running: &Running, account: &Account) -> Reply {
    let now = now_unix();
    match running.engine.vault().open_valid(account, now).await {
        Ok(Some(open)) => Reply {
            status: StatusCode::OK,
            body: json!({
                "client_id": account.client_id,
                "access_token": open.access_token,
                "obtained_at": ist_iso(open.obtained_at),
                "expires_at": ist_iso(open.expires_at),
                "expires_in": open.expires_at - now,
            }),
            retry_after: None,
            outcome: Outcome::Served,
            reason: format!("valid for {}", describe(open.expires_at - now)),
        },
        Ok(None) => {
            let wait = running.window.seconds_left(now).max(
                running
                    .window
                    .seconds_until_open(now)
                    .min(NOT_READY_RETRY_SECONDS),
            );
            Reply::refused(
                StatusCode::SERVICE_UNAVAILABLE,
                "no_token_yet",
                "no valid token for this account yet",
            )
            .retry_in(wait.clamp(30, NOT_READY_RETRY_SECONDS))
        }
        Err(error) => {
            tracing::error!(alias = %account.alias, %error, "a stored token would not open");
            Reply::refused(
                StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
                "the stored token would not open",
            )
        }
    }
}
