use std::net::SocketAddr;

use axum::Extension;
use axum::Json;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};

use super::guard::Refusal;
use super::{Context, Running};
use crate::clock::{describe, ist_iso, now_unix};
use crate::config::Account;
use crate::otp::now_seconds;
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

struct Reply {
    status: StatusCode,
    body: Value,
    retry_after: Option<i64>,
    outcome: &'static str,
    reason: String,
}

impl Reply {
    fn refused(status: StatusCode, error: &str, reason: impl Into<String>) -> Self {
        Self {
            status,
            body: json!({ "error": error }),
            retry_after: None,
            outcome: "refused",
            reason: reason.into(),
        }
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
    body: Option<Json<TokenRequest>>,
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
    let request = body.map(|Json(request)| request);
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

    let record = RequestRecord {
        at: now_unix(),
        client_id: if valid_client_id(&client_id) {
            client_id.clone()
        } else {
            "<invalid>".into()
        },
        alias: account.map(|account| account.alias.clone()),
        ip: ip.clone(),
        outcome: reply.outcome.into(),
        reason: reply.reason.clone(),
    };
    if let Err(error) = running.engine.vault().store().record_request(&record).await {
        tracing::warn!(%error, "could not record a token request");
    }
    let live = running.engine.live();
    let shown = if valid_client_id(&client_id) {
        client_id_hint(&client_id)
    } else {
        "an invalid client".into()
    };
    if reply.outcome == "served" {
        if let Some(account) = account {
            live.update(&account.alias, |view| view.last_served_at = Some(record.at));
        }
        live.note(
            EventKind::Api,
            account.map(|account| account.alias.as_str()),
            format!(
                "token served to {shown} from {}",
                ip.as_deref().unwrap_or("?")
            ),
        );
    } else {
        live.note(
            EventKind::Api,
            account.map(|account| account.alias.as_str()),
            format!("token refused for {shown}: {}", reply.reason),
        );
    }
    reply.into_response()
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
    if running.api.require_access_jwt && headers.get(ACCESS_JWT_HEADER).is_none() {
        return Reply::refused(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "no Cloudflare Access service token",
        );
    }
    if !context
        .guard
        .admit_ip(ip.unwrap_or("unknown"), now_seconds())
    {
        let mut reply = Reply::refused(
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limited",
            "too many requests from this IP",
        );
        reply.retry_after = Some(60);
        return reply;
    }
    let Some(request) = request else {
        return Reply::refused(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "the body is not {client_id, totp} JSON",
        );
    };
    let code = request.totp.as_deref().unwrap_or_default();
    if !valid_client_id(client_id) || !crate::otp::is_six_digits(code) {
        return Reply::refused(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "client_id or totp is malformed",
        );
    }

    let seed = account.map(|account| account.api_totp_seed.as_slice());
    match context
        .guard
        .check_code(client_id, seed, code, now_seconds())
    {
        Ok(_) => {}
        Err(Refusal::Malformed) => {
            return Reply::refused(StatusCode::BAD_REQUEST, "bad_request", "totp is malformed");
        }
        Err(Refusal::Reused) => {
            let mut reply = Reply::refused(
                StatusCode::CONFLICT,
                "code_already_used",
                "a code is accepted once; wait for the next one",
            );
            reply.retry_after =
                Some(i64::try_from(crate::otp::seconds_remaining(now_seconds())).unwrap_or(30));
            return reply;
        }
        Err(Refusal::Locked { until }) => {
            let mut reply = Reply::refused(
                StatusCode::TOO_MANY_REQUESTS,
                "locked",
                "locked after repeated wrong codes",
            );
            reply.retry_after =
                Some(i64::try_from(until.saturating_sub(now_seconds())).unwrap_or(600));
            return reply;
        }
        Err(Refusal::Wrong { locked_until }) => {
            if let Some(until) = locked_until {
                running.engine.notifier().send(
                    &Notice::problem(Topic::Api, "a client was locked out of the token API")
                        .fact("client", client_id_hint(client_id))
                        .fact("known account", account.map_or("no", |_| "yes"))
                        .fact("from", ip.unwrap_or("?"))
                        .fact(
                            "locked for",
                            describe(
                                i64::try_from(until.saturating_sub(now_seconds())).unwrap_or(0),
                            ),
                        )
                        .next(
                            "if this was not one of your engines, rotate that account's API seed",
                        ),
                );
            }
            return Reply::refused(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "wrong code or unknown client",
            );
        }
    }

    let account = account.expect("a passing code implies a known account");
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
            outcome: "served",
            reason: format!("valid for {}", describe(open.expires_at - now)),
        },
        Ok(None) => {
            let mut reply = Reply::refused(
                StatusCode::SERVICE_UNAVAILABLE,
                "no_token_yet",
                "no valid token for this account yet",
            );
            let wait = running.window.seconds_left(now).max(
                running
                    .window
                    .seconds_until_open(now)
                    .min(NOT_READY_RETRY_SECONDS),
            );
            reply.retry_after = Some(wait.clamp(30, NOT_READY_RETRY_SECONDS));
            reply
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
