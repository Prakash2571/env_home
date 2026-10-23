use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{Value, json};

use crate::access::JWT_HEADER;
use crate::api_harness::*;
use crate::otp::{code_at, decode_base32, now_seconds};

fn send(harness: &Harness, method: &str, path: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(path)
        .header(JWT_HEADER, harness.signed())
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn form(alias: &str, client_id: &str) -> Value {
    json!({
        "alias": alias,
        "client_id": client_id,
        "api_key": "dhan-api-key-alt",
        "api_secret": "dhan-api-secret-alt",
        "account_id": "8888888888",
        "pin": "654321",
        "dhan_totp_secret": "JBSWY3DPEHPK3PXPJBSWY3DP",
    })
}

async fn state(harness: &Harness) -> Value {
    let request = Request::get("/api/state")
        .header(JWT_HEADER, harness.signed())
        .body(Body::empty())
        .unwrap();
    call(&harness.router, request).await.2
}

async fn fetch(harness: &Harness, client_id: &str, seed: &[u8]) -> (StatusCode, Value) {
    let request = token_request(
        json!({"client_id": client_id, "totp": code_at(seed, now_seconds())}),
        "198.51.100.20",
    );
    let (status, _, body) = call(&harness.router, request).await;
    (status, body)
}

#[tokio::test]
async fn an_added_account_shows_masked_serves_the_api_and_its_seed_is_shown_once() {
    let harness = harness(open_api(), access_required(), None).await;
    let (status, headers, added) = call(
        &harness.router,
        send(&harness, "POST", "/api/accounts", form("Alt", "1100000002")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(headers["cache-control"], "no-store");
    assert_eq!(added["alias"], "alt");
    let seed = decode_base32(added["token_api_seed"].as_str().unwrap()).unwrap();
    assert!(
        added["otpauth_uri"]
            .as_str()
            .unwrap()
            .starts_with("otpauth://totp/BrokerLogin:alt?secret=")
    );

    let snapshot = state(&harness).await;
    assert_eq!(snapshot["accounts_loaded"], true);
    let aliases: Vec<&str> = snapshot["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|view| view["alias"].as_str().unwrap())
        .collect();
    assert_eq!(aliases, ["main", "alt"]);
    let text = snapshot.to_string();
    for secret in [
        "dhan-api-key-alt",
        "dhan-api-secret-alt",
        "654321",
        "1100000002",
        added["token_api_seed"].as_str().unwrap(),
    ] {
        assert!(
            !text.contains(secret),
            "the dashboard state carries {secret}"
        );
    }

    let (status, body) = fetch(&harness, "1100000002", &seed).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::SERVICE_UNAVAILABLE, Some("no_token_yet")),
        "the new account is known to the token API at once"
    );
}

#[tokio::test]
async fn bad_forms_duplicates_and_unsigned_requests_are_refused() {
    let harness = harness(open_api(), access_required(), None).await;
    let router = &harness.router;

    let mut bad = form("alt", "1100000002");
    bad["pin"] = json!("12ab");
    bad["dhan_totp_secret"] = json!("not base32!");
    let (status, _, body) = call(router, send(&harness, "POST", "/api/accounts", bad)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let fields: Vec<&str> = body["problems"]
        .as_array()
        .unwrap()
        .iter()
        .map(|problem| problem["field"].as_str().unwrap())
        .collect();
    assert_eq!(fields, ["pin", "dhan_totp_secret"]);
    assert!(!body.to_string().contains("12ab"));

    let (status, _, body) = call(
        router,
        send(
            &harness,
            "POST",
            "/api/accounts",
            form("main", "1100000009"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["error"].as_str().unwrap().contains("alias"));
    let (status, _, body) = call(
        router,
        send(&harness, "POST", "/api/accounts", form("alt", "1100000001")),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(body["error"].as_str().unwrap().contains("client ID"));
    let (status, _, _) = call(
        router,
        send(&harness, "POST", "/api/accounts", json!({"alias": "x"})),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a form missing fields");

    let unsigned = Request::post("/api/accounts")
        .header("content-type", "application/json")
        .header("cf-access-authenticated-user-email", EMAIL)
        .body(Body::from(form("alt", "1100000002").to_string()))
        .unwrap();
    assert_eq!(call(router, unsigned).await.0, StatusCode::UNAUTHORIZED);
    let not_json = Request::post("/api/accounts")
        .header(JWT_HEADER, harness.signed())
        .body(Body::from(form("alt", "1100000002").to_string()))
        .unwrap();
    assert_eq!(
        call(router, not_json).await.0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert_eq!(
        state(&harness).await["accounts"].as_array().unwrap().len(),
        1
    );
}

#[tokio::test]
async fn removing_needs_the_alias_typed_waits_for_a_login_and_stops_the_api_at_once() {
    let harness = harness(open_api(), access_required(), None).await;
    let router = &harness.router;
    let seed = crate::test_support::account("main", "1").api_totp_seed;

    let (status, _, _) = call(
        router,
        send(
            &harness,
            "DELETE",
            "/api/accounts/main",
            json!({"confirm": "mian"}),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "the alias must be typed exactly"
    );

    let lease = harness.engine.registry().begin_login("main").unwrap();
    let (status, _, _) = call(
        router,
        send(
            &harness,
            "DELETE",
            "/api/accounts/main",
            json!({"confirm": "main"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "not while it logs in");
    drop(lease);

    let (status, _, body) = call(
        router,
        send(
            &harness,
            "DELETE",
            "/api/accounts/main",
            json!({"confirm": "main"}),
        ),
    )
    .await;
    assert_eq!((status, &body["removed"]), (StatusCode::OK, &json!("main")));
    assert!(
        state(&harness).await["accounts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let (status, _) = fetch(&harness, "1100000001", &seed).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a removed account fetches nothing"
    );

    let (status, _, _) = call(
        router,
        send(
            &harness,
            "DELETE",
            "/api/accounts/main",
            json!({"confirm": "main"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_new_token_seed_is_shown_once_and_the_old_one_stops_working() {
    let harness = harness(open_api(), access_required(), None).await;
    let old = crate::test_support::account("main", "1").api_totp_seed;

    let (status, _, _) = call(
        &harness.router,
        send(&harness, "POST", "/api/accounts/main/token-seed", json!({})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "confirm by typing the alias"
    );
    let (status, headers, rotated) = call(
        &harness.router,
        send(
            &harness,
            "POST",
            "/api/accounts/main/token-seed",
            json!({"confirm": "main"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "no-store");
    let fresh = decode_base32(rotated["token_api_seed"].as_str().unwrap()).unwrap();
    assert_ne!(fresh, old);

    let (status, _) = fetch(&harness, "1100000001", &old).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "the old seed no longer works"
    );
    let (status, body) = fetch(&harness, "1100000001", &fresh).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::SERVICE_UNAVAILABLE, Some("no_token_yet"))
    );
    let (status, _, _) = call(
        &harness.router,
        send(
            &harness,
            "POST",
            "/api/accounts/nobody/token-seed",
            json!({"confirm": "nobody"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
