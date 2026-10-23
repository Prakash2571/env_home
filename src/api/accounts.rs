//! Adding, removing and re-keying Dhan accounts from the dashboard. Every
//! route needs a verified Access sign-in and a JSON body; a removal or a new
//! seed also needs the alias typed back, so a stray click changes nothing.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;

use super::Context;
use super::admin::admin;
use crate::accounts::{AccountError, AccountForm, Added};
use crate::state::client_id_hint;
use crate::telegram::{Notice, Topic};

#[derive(Deserialize)]
pub struct Confirm {
    confirm: Option<String>,
}

fn reply(status: StatusCode, body: serde_json::Value) -> Response {
    (status, [(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

fn refuse(status: StatusCode, message: &str) -> Response {
    reply(status, json!({ "error": message }))
}

fn failed(error: AccountError) -> Response {
    match error {
        AccountError::Invalid(problems) => reply(
            StatusCode::BAD_REQUEST,
            json!({ "error": "some fields need fixing", "problems": problems }),
        ),
        AccountError::DuplicateAlias => refuse(StatusCode::CONFLICT, "that alias is taken"),
        AccountError::DuplicateClientId => refuse(
            StatusCode::CONFLICT,
            "another account already uses that client ID",
        ),
        AccountError::NotFound => refuse(StatusCode::NOT_FOUND, "no such account"),
        AccountError::Busy => refuse(
            StatusCode::CONFLICT,
            "a login is running for this account; try again when it ends",
        ),
        AccountError::NotLoaded => refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "the stored accounts are still loading; try again in a moment",
        ),
        AccountError::Storage(error) => {
            tracing::error!(%error, "an account change could not be stored");
            refuse(
                StatusCode::INTERNAL_SERVER_ERROR,
                "the change could not be stored; nothing was changed",
            )
        }
    }
}

/// Parsed here rather than by an extractor: serde's errors can quote the
/// value they failed on, and a form value may be a secret.
fn parse<T: for<'de> Deserialize<'de>>(body: &Bytes) -> Option<T> {
    serde_json::from_slice(body).ok()
}

fn confirmed(alias: &str, body: &Bytes) -> bool {
    parse::<Confirm>(body)
        .and_then(|body| body.confirm)
        // Aliases are lower case; a capital from the label or a phone keyboard
        // is the same alias, not a different one.
        .is_some_and(|typed| typed.trim().to_ascii_lowercase() == alias)
}

fn who(email: Option<&String>) -> &str {
    email.map_or("the dashboard", String::as_str)
}

pub async fn add(State(context): State<Context>, headers: HeaderMap, body: Bytes) -> Response {
    let (running, email) = match admin(&context, &headers, true).await {
        Ok(found) => found,
        Err(refusal) => return refusal.into_response(),
    };
    let Some(form) = parse::<AccountForm>(&body) else {
        return refuse(
            StatusCode::BAD_REQUEST,
            "send alias, client_id, api_key, api_secret, account_id, pin and dhan_totp_secret",
        );
    };
    match running.book.add(&form, email.as_deref()).await {
        Ok(added) => {
            let client = running
                .book
                .registry()
                .get(&added.alias)
                .map(|account| client_id_hint(&account.client_id))
                .unwrap_or_default();
            running.engine.notifier().send(
                &Notice::info(Topic::System, format!("account {} added", added.alias))
                    .fact("client", client)
                    .fact("by", who(email.as_ref()))
                    .next("set its token API seed in the trading engine; the dashboard showed it once"),
            );
            shown_once(StatusCode::CREATED, &added)
        }
        Err(error) => failed(error),
    }
}

pub async fn remove(
    State(context): State<Context>,
    Path(alias): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let (running, email) = match admin(&context, &headers, true).await {
        Ok(found) => found,
        Err(refusal) => return refusal.into_response(),
    };
    if !confirmed(&alias, &body) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "type the account's alias as {\"confirm\": \"<alias>\"} to remove it",
        );
    }
    match running.book.remove(&alias, email.as_deref()).await {
        Ok(account) => {
            running.engine.notifier().send(
                &Notice::problem(Topic::System, format!("account {alias} removed"))
                    .fact("client", client_id_hint(&account.client_id))
                    .fact("by", who(email.as_ref()))
                    .next(
                        "its secrets and stored tokens are deleted; engines asking for it get 401",
                    ),
            );
            reply(StatusCode::OK, json!({ "removed": alias }))
        }
        Err(error) => failed(error),
    }
}

pub async fn rotate_seed(
    State(context): State<Context>,
    Path(alias): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let (running, email) = match admin(&context, &headers, true).await {
        Ok(found) => found,
        Err(refusal) => return refusal.into_response(),
    };
    if !confirmed(&alias, &body) {
        return refuse(
            StatusCode::BAD_REQUEST,
            "type the account's alias as {\"confirm\": \"<alias>\"} to replace its seed",
        );
    }
    match running
        .book
        .rotate_token_seed(&alias, email.as_deref())
        .await
    {
        Ok(added) => {
            running.engine.notifier().send(
                &Notice::problem(Topic::System, format!("token API seed of {alias} replaced"))
                    .fact("by", who(email.as_ref()))
                    .next("engines using the old seed get 401 until they are given the new one"),
            );
            shown_once(StatusCode::OK, &added)
        }
        Err(error) => failed(error),
    }
}

fn shown_once(status: StatusCode, added: &Added) -> Response {
    reply(status, serde_json::to_value(added).unwrap_or_default())
}
