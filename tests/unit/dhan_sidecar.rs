use std::sync::Mutex;

use axum::Router;
use axum::routing::post;
use serde_json::{Value, json};

use super::*;
use crate::test_support::{account, serve};

async fn fake_sidecar(reply: Value, seen: Arc<Mutex<Option<Value>>>) -> String {
    let router = Router::new().route(
        "/login",
        post(move |axum::Json(body): axum::Json<Value>| {
            let reply = reply.clone();
            let seen = Arc::clone(&seen);
            async move {
                *seen.lock().unwrap() = Some(body);
                axum::Json(reply)
            }
        }),
    );
    format!("{}/login", serve(router).await)
}

#[tokio::test]
async fn the_sidecar_gets_the_account_pin_and_a_current_code_and_returns_the_token_id() {
    let seen = Arc::new(Mutex::new(None));
    let endpoint = fake_sidecar(
        json!({"ok": true, "tokenId": "tid-1", "stage": "callback"}),
        Arc::clone(&seen),
    )
    .await;
    let sidecar = Sidecar::with_code_lifetime(&endpoint, 0).unwrap();
    let account = account("main", "1100000001");
    let token = sidecar
        .drive(&account, "https://auth/consent", "https://x/cb")
        .await
        .unwrap();
    assert_eq!(token, "tid-1");

    let body = seen.lock().unwrap().clone().unwrap();
    assert_eq!(body["accountId"], "9999999999");
    assert_eq!(body["pin"], "123456");
    assert_eq!(body["consentUrl"], "https://auth/consent");
    let code = body["totp"].as_str().unwrap();
    assert!(crate::otp::is_six_digits(code));
    let now = now_seconds();
    assert!(
        [now, now.saturating_sub(30)]
            .iter()
            .any(|at| code_at(&account.dhan_totp_seed, *at) == code),
        "the code must come from the account's Dhan seed, not the API seed"
    );
}

#[tokio::test]
async fn a_refusal_carries_its_stage_and_whether_another_try_can_pass() {
    let endpoint = fake_sidecar(
        json!({"ok": false, "stage": "totp-rejected", "error": "Dhan said: Too many attempts"}),
        Arc::new(Mutex::new(None)),
    )
    .await;
    let error = Sidecar::with_code_lifetime(&endpoint, 0)
        .unwrap()
        .drive(&account("main", "1"), "c", "r")
        .await
        .unwrap_err();
    let failure = error.downcast_ref::<SidecarFailure>().unwrap();
    assert_eq!(failure.stage, "totp-rejected");
    assert!(
        failure.hopeless,
        "a lockout must stop the retries for the day"
    );
    assert!(error.to_string().contains("failed at totp-rejected"));
}

#[tokio::test]
async fn success_without_a_token_id_or_an_unreadable_reply_is_an_error() {
    let endpoint = fake_sidecar(json!({"ok": true}), Arc::new(Mutex::new(None))).await;
    assert!(
        Sidecar::with_code_lifetime(&endpoint, 0)
            .unwrap()
            .drive(&account("m", "1"), "c", "r")
            .await
            .is_err()
    );

    let silent = Sidecar::with_code_lifetime("http://127.0.0.1:1/login", 0).unwrap();
    assert!(
        silent
            .drive(&account("m", "1"), "c", "r")
            .await
            .unwrap_err()
            .to_string()
            .contains("did not answer")
    );

    let html = serve(Router::new().route("/login", post(|| async { "<html>" }))).await;
    let error = Sidecar::with_code_lifetime(&format!("{html}/login"), 0)
        .unwrap()
        .drive(&account("m", "1"), "c", "r")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("unreadable body"));
}

#[test]
fn sms_otp_and_lockouts_are_hopeless_and_a_wrong_code_is_not() {
    assert!(hopeless("otp", "Dhan asked for an SMS OTP"));
    assert!(hopeless("pin-rejected", "Dhan said: Account locked"));
    assert!(!hopeless("totp-rejected", "Dhan said: Invalid TOTP"));
    assert!(!hopeless("consent", "timeout"));
}

#[test]
fn a_code_is_sent_only_with_enough_life_left_and_never_twice() {
    let start = 1_700_000_010;
    assert_eq!(start % 30, 0, "the fixture sits on a step boundary");
    assert_eq!(wait_for_fresh_code(None, start, 25), 0);
    assert_eq!(
        wait_for_fresh_code(None, start + 10, 25),
        21,
        "20s left is too little, wait for the next step"
    );
    assert_eq!(
        wait_for_fresh_code(Some(step_of(start)), start + 1, 25),
        30,
        "the step was already used"
    );
    assert_eq!(wait_for_fresh_code(Some(step_of(start) - 1), start, 25), 0);
}
