use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use serde_json::{Value, json};

use super::*;
use crate::test_support::{account, serve};

#[derive(Clone, Default)]
struct Seen {
    headers: Arc<Mutex<Vec<(String, String)>>>,
}

async fn fake_dhan(consent: Value, token: (StatusCode, Value)) -> (String, Seen) {
    let seen = Seen::default();
    let router = Router::new()
        .route(
            "/app/generate-consent",
            post(
                move |State(seen): State<Seen>,
                      headers: HeaderMap,
                      Query(query): Query<Vec<(String, String)>>| {
                    let consent = consent.clone();
                    async move {
                        let mut log = seen.headers.lock().unwrap();
                        log.push(("app_id".into(), headers["app_id"].to_str().unwrap().into()));
                        log.push((
                            "app_secret".into(),
                            headers["app_secret"].to_str().unwrap().into(),
                        ));
                        log.extend(query);
                        axum::Json(consent)
                    }
                },
            ),
        )
        .route(
            "/app/consumeApp-consent",
            get(move || {
                let token = token.clone();
                async move { (token.0, axum::Json(token.1)) }
            }),
        )
        .with_state(seen.clone());
    (serve(router).await, seen)
}

fn future_expiry() -> String {
    let later = OffsetDateTime::now_utc() + time::Duration::hours(20);
    later.format(&Rfc3339).unwrap()
}

#[tokio::test]
async fn a_consent_becomes_the_login_url_with_the_account_credentials_as_headers() {
    let (base, seen) = fake_dhan(
        json!({"consentAppId": "abc-123", "status": "success", "consentAppStatus": "GENERATED"}),
        (StatusCode::OK, json!({})),
    )
    .await;
    let auth = DhanAuth::new(&format!("{base}/")).unwrap();
    let url = auth
        .generate_consent(&account("main", "1100000001"))
        .await
        .unwrap();
    assert_eq!(
        url,
        format!("{base}/login/consentApp-login?consentAppId=abc-123")
    );
    let log = seen.headers.lock().unwrap().clone();
    assert!(log.contains(&("app_id".into(), "key".into())));
    assert!(log.contains(&("app_secret".into(), "secret".into())));
    assert!(log.contains(&("client_id".into(), "1100000001".into())));
}

#[tokio::test]
async fn a_consent_that_is_not_generated_is_refused() {
    let (base, _) = fake_dhan(
        json!({"consentAppId": "abc", "status": "success", "consentAppStatus": "PENDING"}),
        (StatusCode::OK, json!({})),
    )
    .await;
    let error = DhanAuth::new(&base)
        .unwrap()
        .generate_consent(&account("main", "1"))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("did not generate"));
}

#[tokio::test]
async fn a_token_is_exchanged_and_checked_against_the_account() {
    let (base, _) = fake_dhan(
        json!({}),
        (StatusCode::OK, json!({"dhanClientId": "1100000001", "accessToken": "eyJ.token", "expiryTime": future_expiry()})),
    )
    .await;
    let auth = DhanAuth::new(&base).unwrap();
    let session = auth
        .exchange_token(&account("main", "1100000001"), "tok-1")
        .await
        .unwrap();
    assert_eq!(session.access_token, "eyJ.token");
    assert!(session.expires_at > session.obtained_at);

    let error = auth
        .exchange_token(&account("alt", "1100000002"), "tok-1")
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("different client ID"),
        "a token for another account must never be stored"
    );
}

#[tokio::test]
async fn dhan_errors_are_reported_with_their_code_and_clipped() {
    let long = "x ".repeat(200);
    let (base, _) = fake_dhan(
        json!({}),
        (
            StatusCode::BAD_REQUEST,
            json!({"errorCode": "DH-905", "errorMessage": long}),
        ),
    )
    .await;
    let error = DhanAuth::new(&base)
        .unwrap()
        .exchange_token(&account("main", "1"), "t")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("HTTP 400; DH-905"));
    assert!(error.len() < 200);
}

#[tokio::test]
async fn an_unreachable_dhan_and_a_bad_token_id_fail_cleanly() {
    let auth = DhanAuth::new("http://127.0.0.1:1").unwrap();
    assert!(
        auth.generate_consent(&account("main", "1"))
            .await
            .unwrap_err()
            .to_string()
            .contains("request failed")
    );
    assert!(
        auth.exchange_token(&account("main", "1"), " ")
            .await
            .is_err()
    );
    assert!(
        auth.exchange_token(&account("main", "1"), "a\nb")
            .await
            .is_err()
    );
}

#[test]
fn expiry_parses_rfc3339_and_dhans_naive_ist_form() {
    assert_eq!(parse_expiry("2026-10-22T08:00:00Z").unwrap(), 1_792_656_000);
    assert_eq!(
        parse_expiry("2026-10-22T13:30:00").unwrap(),
        1_792_656_000,
        "a naive time is IST"
    );
    assert_eq!(
        parse_expiry("2026-10-22T13:30:00.250").unwrap(),
        1_792_656_000
    );
    assert!(parse_expiry("tomorrow").is_err());
}

#[test]
fn a_session_must_be_valid_for_more_than_the_margin() {
    let account = account("main", "1");
    let response = |expiry: &str, token: &str| TokenResponse {
        client_id: "1".into(),
        access_token: token.into(),
        expiry_time: expiry.into(),
    };
    let now = 1_792_656_000 - 30;
    assert!(validate_session(&account, response("2026-10-22T08:00:00Z", "t"), now).is_err());
    assert!(
        validate_session(&account, response("2026-10-22T09:00:00Z", "has space"), now).is_err()
    );
    assert!(validate_session(&account, response("2026-10-22T09:00:00Z", "t"), now).is_ok());
}

#[test]
fn error_bodies_without_a_code_are_ignored() {
    assert_eq!(dhan_error(br#"{"errorMessage":"only"}"#), None);
    assert_eq!(dhan_error(b"not json"), None);
    assert_eq!(
        dhan_error(br#"{"errorType":"Input_Exception"}"#).as_deref(),
        Some("Input_Exception")
    );
}
