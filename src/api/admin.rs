use std::convert::Infallible;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::{Stream, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tokio_stream::wrappers::WatchStream;

use super::{Context, Running, snapshot};
use crate::access::JWT_HEADER;
use crate::clock::now_unix;
use crate::config::AdminPolicy;
use crate::login::{Drive, LoginRequest};
use crate::state::EventKind;
use crate::telegram::{Notice, Topic};

const STREAM_KEEPALIVE: Duration = Duration::from_secs(15);

fn refuse(status: StatusCode, message: &str) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({ "error": message })),
    )
        .into_response()
}

pub struct Denied(StatusCode, &'static str);

impl IntoResponse for Denied {
    fn into_response(self) -> Response {
        refuse(self.0, self.1)
    }
}

/// Who is using the dashboard, from the JWT Cloudflare Access signed. The
/// plain email header Access also sends is never trusted: any process on the
/// box could set it.
pub async fn identity(headers: &HeaderMap, policy: &AdminPolicy) -> Result<Option<String>, Denied> {
    if !policy.require_access_identity {
        return Ok(None);
    }
    let Some(check) = &policy.access else {
        return Err(Denied(
            StatusCode::SERVICE_UNAVAILABLE,
            "Cloudflare Access verification is not configured",
        ));
    };
    let Some(token) = headers
        .get(JWT_HEADER)
        .and_then(|value| value.to_str().ok())
    else {
        return Err(Denied(
            StatusCode::UNAUTHORIZED,
            "sign in through Cloudflare Access",
        ));
    };
    let signed_in = match check
        .verifier
        .verify(token, &check.audience, now_unix())
        .await
    {
        Ok(signed_in) => signed_in,
        Err(error) => {
            tracing::info!(reason = error.reason(), "a dashboard request was refused");
            return Err(Denied(
                StatusCode::UNAUTHORIZED,
                "sign in through Cloudflare Access",
            ));
        }
    };
    let Some(email) = signed_in.email else {
        return Err(Denied(
            StatusCode::FORBIDDEN,
            "only a signed-in person may use the dashboard",
        ));
    };
    if !policy.allowed_emails.is_empty() && !policy.allowed_emails.contains(&email) {
        return Err(Denied(
            StatusCode::FORBIDDEN,
            "this account may not use the dashboard",
        ));
    }
    Ok(Some(email))
}

fn is_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().starts_with("application/json"))
}

pub(super) async fn admin<'context>(
    context: &'context Context,
    headers: &HeaderMap,
    mutating: bool,
) -> Result<(&'context Running, Option<String>), Denied> {
    let Some(running) = &context.running else {
        return Err(Denied(
            StatusCode::SERVICE_UNAVAILABLE,
            "the service is not configured",
        ));
    };
    let who = identity(headers, &running.admin).await?;
    if mutating && !is_json(headers) {
        return Err(Denied(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "send application/json",
        ));
    }
    Ok((running, who))
}

pub async fn state(State(context): State<Context>, headers: HeaderMap) -> Response {
    let policy = context
        .running
        .as_ref()
        .map_or(&context.admin, |running| &running.admin);
    if let Err(refusal) = identity(&headers, policy).await {
        return refusal.into_response();
    }
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(snapshot(&context).await),
    )
        .into_response()
}

fn events(context: Context) -> impl Stream<Item = Result<Event, Infallible>> {
    let revisions = context
        .running
        .as_ref()
        .map(|running| running.engine.live().subscribe());
    let changes = match revisions {
        Some(receiver) => WatchStream::new(receiver).map(|_| ()).boxed(),
        None => futures_util::stream::once(async {}).boxed(),
    };
    changes.then(move |()| {
        let context = context.clone();
        async move {
            let body =
                serde_json::to_string(&snapshot(&context).await).unwrap_or_else(|_| "{}".into());
            Ok(Event::default().event("snapshot").data(body))
        }
    })
}

pub async fn stream(State(context): State<Context>, headers: HeaderMap) -> Response {
    let policy = context
        .running
        .as_ref()
        .map_or(&context.admin, |running| &running.admin);
    if let Err(refusal) = identity(&headers, policy).await {
        return refusal.into_response();
    }
    let sse = Sse::new(events(context)).keep_alive(KeepAlive::new().interval(STREAM_KEEPALIVE));
    (
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::HeaderName::from_static("x-accel-buffering"), "no"),
        ],
        sse,
    )
        .into_response()
}

#[derive(Deserialize)]
pub struct LoginBody {
    drive: Drive,
}

fn schedule(
    running: &Running,
    alias: Option<String>,
    drive: Drive,
    who: Option<String>,
) -> Response {
    if drive == Drive::Scheduled {
        return refuse(StatusCode::BAD_REQUEST, "choose automated or manual");
    }
    if drive == Drive::Automated && !running.engine.automation_available() {
        return refuse(
            StatusCode::CONFLICT,
            "there is no browser sidecar; use a manual login",
        );
    }
    let target = alias.clone().unwrap_or_else(|| "every account".into());
    if !running.scheduler.request(LoginRequest { alias, drive }) {
        return refuse(
            StatusCode::TOO_MANY_REQUESTS,
            "too many logins are already queued",
        );
    }
    running.engine.live().note(
        EventKind::Login,
        None,
        format!(
            "{} login of {target} requested by {}",
            drive.label(),
            who.as_deref().unwrap_or("the dashboard")
        ),
    );
    (
        StatusCode::ACCEPTED,
        Json(json!({ "queued": target, "drive": drive.label() })),
    )
        .into_response()
}

pub async fn request_login(
    State(context): State<Context>,
    Path(alias): Path<String>,
    headers: HeaderMap,
    body: Option<Json<LoginBody>>,
) -> Response {
    let (running, who) = match admin(&context, &headers, true).await {
        Ok(found) => found,
        Err(refusal) => return refusal.into_response(),
    };
    let Some(Json(body)) = body else {
        return refuse(
            StatusCode::BAD_REQUEST,
            "send {\"drive\": \"automated\" | \"manual\"}",
        );
    };
    if running.engine.account(&alias).is_none() {
        return refuse(StatusCode::NOT_FOUND, "no such account");
    }
    schedule(running, Some(alias), body.drive, who)
}

pub async fn request_login_all(
    State(context): State<Context>,
    headers: HeaderMap,
    body: Option<Json<LoginBody>>,
) -> Response {
    let (running, who) = match admin(&context, &headers, true).await {
        Ok(found) => found,
        Err(refusal) => return refusal.into_response(),
    };
    let Some(Json(body)) = body else {
        return refuse(
            StatusCode::BAD_REQUEST,
            "send {\"drive\": \"automated\" | \"manual\"}",
        );
    };
    schedule(running, None, body.drive, who)
}

pub async fn test_telegram(State(context): State<Context>, headers: HeaderMap) -> Response {
    let (running, who) = match admin(&context, &headers, true).await {
        Ok(found) => found,
        Err(refusal) => return refusal.into_response(),
    };
    let notifier = running.engine.notifier();
    if !notifier.view().configured {
        return refuse(StatusCode::CONFLICT, "Telegram is not configured");
    }
    notifier.send(
        &Notice::info(Topic::System, "test notice from the dashboard")
            .fact("sent by", who.as_deref().unwrap_or("the dashboard")),
    );
    (StatusCode::ACCEPTED, Json(json!({ "queued": true }))).into_response()
}
