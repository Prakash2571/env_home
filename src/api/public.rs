use std::net::SocketAddr;

use axum::Extension;
use axum::extract::{ConnectInfo, RawQuery, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use serde_json::json;

use super::{Context, VERSION};
use crate::dhan::callback::Delivery;
use crate::state::EventKind;

const ROBOTS: &str = "User-agent: *\nDisallow: /\n";

pub async fn health(State(context): State<Context>) -> Response {
    let status = if context.running.is_some() {
        "ok"
    } else {
        "unconfigured"
    };
    (
        [(header::CACHE_CONTROL, "no-store")],
        axum::Json(
            json!({ "status": status, "version": VERSION, "problems": context.problems.len() }),
        ),
    )
        .into_response()
}

pub async fn robots() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        ROBOTS,
    )
}

fn page(title: &str, body: &str) -> Html<String> {
    Html(format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" \
         content=\"width=device-width,initial-scale=1\"><title>{title}</title></head><body \
         style=\"font-family:system-ui,sans-serif;background:#0b0f14;color:#e6edf3;display:grid;\
         place-items:center;min-height:100vh;margin:0\"><main style=\"max-width:28rem;padding:1.5rem\">\
         <h1 style=\"font-size:1.25rem\">{title}</h1><p>{body}</p></main></body></html>"
    ))
}

pub async fn dhan_callback(
    State(context): State<Context>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Response {
    let Some(running) = &context.running else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            page("Not ready", "The service is not configured."),
        )
            .into_response();
    };
    let gate = running.engine.gate();
    let waiting = gate.waiting_for();
    let source = super::token::client_ip(
        &headers,
        peer.map(|Extension(ConnectInfo(address))| address),
        running.api.client_ip_header.as_deref(),
    )
    .unwrap_or_else(|| "unknown".into());
    match gate.deliver(query.as_deref().unwrap_or_default(), &source) {
        Delivery::Accepted => {
            if let Some(alias) = waiting.as_deref() {
                running.engine.live().note(
                    EventKind::Login,
                    Some(alias),
                    "the consent page redirected back",
                );
            }
            (
                [(header::CACHE_CONTROL, "no-store")],
                page(
                    "Login received",
                    "The broker sent the login back. You can close this tab.",
                ),
            )
                .into_response()
        }
        Delivery::NoLoginPending => (
            StatusCode::GONE,
            page(
                "No login waiting",
                "Nothing is waiting for this callback. Start a login from the dashboard.",
            ),
        )
            .into_response(),
        Delivery::Invalid => (
            StatusCode::BAD_REQUEST,
            page("Not a valid callback", "This link did not carry a login."),
        )
            .into_response(),
        Delivery::TooManyAttempts => (
            StatusCode::TOO_MANY_REQUESTS,
            page(
                "Too many attempts",
                "Start a fresh login from the dashboard.",
            ),
        )
            .into_response(),
    }
}
