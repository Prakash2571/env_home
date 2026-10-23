use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;
use crate::access::JWT_HEADER;
use crate::api_harness::*;
use crate::config::{AdminPolicy, ApiPolicy};
use crate::test_support::{ACCESS_API_AUD, ACCESS_AUD, AccessSigner};

#[tokio::test]
async fn an_unconfigured_service_stays_up_and_says_what_is_missing() {
    let access = TestAccess::start().await;
    let (admin, _) = access.arm(access_required(), open_api());
    let context = Arc::new(crate::app::unconfigured(
        vec!["CBL_ENCRYPTION_KEY is missing".into()],
        7,
        admin,
    ));
    let router = router(context, std::path::Path::new("/nonexistent"));
    let (status, headers, body) = call(
        &router,
        Request::get("/health").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "unconfigured");
    assert_eq!(
        headers["x-robots-tag"],
        "noindex, nofollow, noarchive, nosnippet"
    );
    assert_eq!(headers["x-frame-options"], "DENY");
    assert!(
        headers["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );

    let (status, _, _) = call(
        &router,
        Request::get("/api/state").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "even unconfigured, the state names variables, so it needs the Access identity"
    );
    let (_, _, state) = call(
        &router,
        Request::get("/api/state")
            .header(JWT_HEADER, access.person(EMAIL))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(state["configured"], false);
    assert_eq!(state["problems"][0], "CBL_ENCRYPTION_KEY is missing");

    let (status, _, _) = call(&router, token_request(json!({}), "203.0.113.1")).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let (status, _, _) = call(
        &router,
        Request::get("/dhan/callback?tokenId=x")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    let (status, _, robots) = call(
        &router,
        Request::get("/robots.txt").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(robots, "User-agent: *\nDisallow: /\n");
}

#[tokio::test]
async fn the_dashboard_state_needs_the_access_identity_and_respects_the_allowlist() {
    let harness = harness(open_api(), access_required(), None).await;
    let (status, _, _) = call(
        &harness.router,
        Request::get("/api/state").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let request = Request::get("/api/state")
        .header(JWT_HEADER, harness.signed())
        .body(Body::empty())
        .unwrap();
    let (status, headers, state) = call(&harness.router, request).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["cache-control"], "no-store");
    assert_eq!(state["configured"], true);
    assert_eq!(state["accounts"][0]["alias"], "main");
    assert_eq!(state["accounts"][0]["client_id_hint"], "••••0001");
    assert!(
        !state.to_string().contains("1100000001"),
        "the dashboard never receives a whole client ID"
    );
    assert_eq!(state["window"]["label"], "08:00-08:30 IST");

    let listed = harness_with_allowlist(vec!["someone@else.in".into()]).await;
    let request = Request::get("/api/state")
        .header(JWT_HEADER, listed.signed())
        .body(Body::empty())
        .unwrap();
    assert_eq!(call(&listed.router, request).await.0, StatusCode::FORBIDDEN);
}

async fn harness_with_allowlist(emails: Vec<String>) -> Harness {
    harness(
        open_api(),
        AdminPolicy {
            require_access_identity: true,
            allowed_emails: emails,
            access: None,
        },
        None,
    )
    .await
}

#[tokio::test]
async fn a_right_code_with_no_token_yet_says_when_to_come_back() {
    let harness = harness(open_api(), access_required(), None).await;
    let (status, headers, body) = call(
        &harness.router,
        token_request(
            json!({"client_id": "1100000001", "totp": current_code()}),
            "203.0.113.2",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["error"], "no_token_yet");
    assert!(headers.contains_key("retry-after"));
}

#[tokio::test]
async fn a_right_code_gets_the_token_once_and_a_replay_is_refused() {
    let harness = harness(open_api(), access_required(), None).await;
    seal_token(&harness).await;
    let code = current_code();
    let (status, headers, body) = call(
        &harness.router,
        token_request(
            json!({"client_id": "1100000001", "totp": code}),
            "203.0.113.3",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["access_token"], "eyJ.live");
    assert_eq!(body["client_id"], "1100000001");
    assert!(body["expires_in"].as_i64().unwrap() > 0);
    assert_eq!(headers["cache-control"], "no-store");

    let (status, _, body) = call(
        &harness.router,
        token_request(
            json!({"client_id": "1100000001", "totp": code}),
            "203.0.113.3",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "code_already_used");

    let requests = harness.store.recent_requests(5).await.unwrap();
    assert_eq!(requests[0].outcome, "refused");
    assert_eq!(requests[1].outcome, "served");
    assert_eq!(requests[1].ip.as_deref(), Some("203.0.113.3"));
    assert!(
        harness
            .engine
            .live()
            .account("main")
            .unwrap()
            .last_served_at
            .is_some()
    );
}

#[tokio::test]
async fn wrong_codes_and_unknown_clients_look_the_same_and_both_lock_out() {
    let harness = harness(open_api(), access_required(), None).await;
    seal_token(&harness).await;
    let (wrong, _, wrong_body) = call(
        &harness.router,
        token_request(
            json!({"client_id": "1100000001", "totp": "000000"}),
            "203.0.113.4",
        ),
    )
    .await;
    let (unknown, _, unknown_body) = call(
        &harness.router,
        token_request(
            json!({"client_id": "1100009999", "totp": "000000"}),
            "203.0.113.4",
        ),
    )
    .await;
    assert_eq!(
        (wrong, unknown),
        (StatusCode::UNAUTHORIZED, StatusCode::UNAUTHORIZED)
    );
    assert_eq!(
        wrong_body, unknown_body,
        "the reply must not reveal whether the client exists"
    );

    for _ in 0..5 {
        call(
            &harness.router,
            token_request(
                json!({"client_id": "1100000001", "totp": "000000"}),
                "203.0.113.5",
            ),
        )
        .await;
    }
    let (status, headers, body) = call(
        &harness.router,
        token_request(
            json!({"client_id": "1100000001", "totp": current_code()}),
            "203.0.113.5",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["error"], "locked");
    assert!(
        headers["retry-after"]
            .to_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > 500
    );

    let (status, _, body) = call(
        &harness.router,
        token_request(
            json!({"client_id": "1100000001", "totp": current_code()}),
            "198.51.100.10",
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the guesser's lockout must not deny the engine on its own IP"
    );
    assert_eq!(body["access_token"], "eyJ.live");
}

#[tokio::test]
async fn malformed_requests_are_refused_before_any_code_is_checked() {
    let harness = harness(open_api(), access_required(), None).await;
    for body in [
        json!({}),
        json!({"client_id": "1100000001"}),
        json!({"client_id": "11 00", "totp": "123456"}),
        json!({"client_id": "1100000001", "totp": "12345"}),
    ] {
        assert_eq!(
            call(&harness.router, token_request(body, "203.0.113.6"))
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    let not_json = Request::post("/api/v1/token")
        .header("cf-connecting-ip", "203.0.113.6")
        .body(Body::from("x"))
        .unwrap();
    assert_eq!(
        call(&harness.router, not_json).await.0,
        StatusCode::BAD_REQUEST
    );
    let requests = harness.store.recent_requests(1).await.unwrap();
    assert_eq!(requests[0].client_id, "<invalid>");
}

#[tokio::test]
async fn one_ip_is_rate_limited_and_the_access_service_token_can_be_required() {
    let limited = harness(open_api(), access_required(), None).await;
    for _ in 0..30 {
        call(&limited.router, token_request(json!({}), "198.51.100.9")).await;
    }
    let (status, headers, _) =
        call(&limited.router, token_request(json!({}), "198.51.100.9")).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(headers["retry-after"], "60");

    let strict = harness(
        ApiPolicy {
            require_access_jwt: true,
            client_ip_header: None,
            access: None,
        },
        access_required(),
        None,
    )
    .await;
    let (status, _, _) = call(
        &strict.router,
        token_request(json!({"client_id": "1100000001", "totp": "123456"}), "x"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "D4: with the flag on, no Access JWT means no token"
    );

    let with = |token: String| {
        Request::post("/api/v1/token")
            .header("content-type", "application/json")
            .header(JWT_HEADER, token)
            .body(Body::from(
                json!({"client_id": "1100000001", "totp": current_code()}).to_string(),
            ))
            .unwrap()
    };
    let (status, _, body) = call(&strict.router, with(strict.access.person(EMAIL))).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("unauthorized")),
        "a dashboard sign-in is not a service token for the API"
    );
    let (status, _, body) = call(&strict.router, with(strict.access.engine())).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (StatusCode::SERVICE_UNAVAILABLE, Some("no_token_yet")),
        "a verified service token reaches the TOTP check"
    );
}

#[tokio::test]
async fn the_dashboard_trusts_only_a_jwt_signed_for_its_own_application() {
    let harness = harness(open_api(), access_required(), None).await;
    let state = |token: Option<String>, forged_email: bool| {
        let mut builder = Request::get("/api/state");
        if forged_email {
            builder = builder.header("cf-access-authenticated-user-email", EMAIL);
        }
        if let Some(token) = token {
            builder = builder.header(JWT_HEADER, token);
        }
        builder.body(Body::empty()).unwrap()
    };
    let now = crate::clock::now_unix();
    let access = &harness.access;
    let cases = [
        (
            state(None, true),
            StatusCode::UNAUTHORIZED,
            "a forged email header alone",
        ),
        (
            state(Some("junk".into()), true),
            StatusCode::UNAUTHORIZED,
            "a junk token",
        ),
        (
            state(
                Some(
                    access
                        .signer
                        .person(&access.team, ACCESS_API_AUD, EMAIL, now),
                ),
                false,
            ),
            StatusCode::UNAUTHORIZED,
            "a sign-in for another application",
        ),
        (
            state(
                Some(AccessSigner::new("other-key").person(&access.team, ACCESS_AUD, EMAIL, now)),
                false,
            ),
            StatusCode::UNAUTHORIZED,
            "a token from a key the team never published",
        ),
        (
            state(
                Some(access.signer.service(&access.team, ACCESS_AUD, now)),
                false,
            ),
            StatusCode::FORBIDDEN,
            "a service token is not a person",
        ),
        (
            state(Some(harness.signed()), false),
            StatusCode::OK,
            "the operator",
        ),
    ];
    for (request, expected, case) in cases {
        assert_eq!(call(&harness.router, request).await.0, expected, "{case}");
    }
}

fn login_request(token: &str, path: &str, body: Value, json_type: bool) -> Request<Body> {
    let builder = Request::post(path).header(JWT_HEADER, token);
    let builder = if json_type {
        builder.header("content-type", "application/json")
    } else {
        builder
    };
    builder.body(Body::from(body.to_string())).unwrap()
}

#[tokio::test]
async fn dashboard_logins_are_queued_and_validated() {
    let harness = harness(open_api(), access_required(), None).await;
    let router = &harness.router;
    assert_eq!(
        call(
            router,
            login_request(
                &harness.signed(),
                "/api/accounts/main/login",
                json!({"drive": "manual"}),
                false
            )
        )
        .await
        .0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert_eq!(
        call(
            router,
            login_request(
                &harness.signed(),
                "/api/accounts/ghost/login",
                json!({"drive": "manual"}),
                true
            )
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            router,
            login_request(
                &harness.signed(),
                "/api/accounts/main/login",
                json!({"drive": "scheduled"}),
                true
            )
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        call(
            router,
            login_request(
                &harness.signed(),
                "/api/accounts/main/login",
                json!({"drive": "automated"}),
                true
            )
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        call(
            router,
            login_request(
                &harness.signed(),
                "/api/accounts/login",
                json!({"nope": 1}),
                true
            )
        )
        .await
        .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let (status, _, body) = call(
        router,
        login_request(
            &harness.signed(),
            "/api/accounts/main/login",
            json!({"drive": "manual"}),
            true,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["queued"], "main");

    for _ in 0..100 {
        if harness.engine.gate().waiting_for().is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let (status, _, page) = call(
        router,
        Request::get("/dhan/callback?tokenId=t-1")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.as_str().unwrap().contains("Login received"));
    for _ in 0..100 {
        if harness
            .engine
            .vault()
            .latest(&harness.engine.account("main").unwrap())
            .await
            .unwrap()
            .is_some()
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        harness
            .engine
            .vault()
            .latest(&harness.engine.account("main").unwrap())
            .await
            .unwrap()
            .is_some()
    );

    let (status, _, _) = call(
        router,
        Request::get("/dhan/callback?tokenId=late")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::GONE);
    let (status, _, body) = call(
        router,
        login_request(
            &harness.signed(),
            "/api/accounts/login",
            json!({"drive": "manual"}),
            true,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(body["queued"], "every account");
    assert_eq!(
        call(
            router,
            login_request(&harness.signed(), "/api/telegram/test", json!({}), true)
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn the_stream_opens_with_a_snapshot_event() {
    let harness = harness(
        open_api(),
        AdminPolicy {
            require_access_identity: false,
            allowed_emails: Vec::new(),
            access: None,
        },
        None,
    )
    .await;
    let response = harness
        .router
        .clone()
        .oneshot(Request::get("/api/stream").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let mut body = response.into_body();
    let frame = body.frame().await.unwrap().unwrap().into_data().unwrap();
    let text = String::from_utf8_lossy(&frame);
    assert!(text.starts_with("event: snapshot"));
    assert!(text.contains("\"configured\":true"));
}

#[tokio::test]
async fn the_built_console_is_served_with_a_spa_fallback() {
    let dir = std::env::temp_dir().join(format!("cbl-console-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("index.html"),
        "<!doctype html><title>console</title>",
    )
    .unwrap();
    let context = Arc::new(crate::app::unconfigured(Vec::new(), 0, access_required()));
    let router = router(context, &dir);
    let (status, _, body) = call(
        &router,
        Request::get("/accounts/main").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.as_str().unwrap().contains("<title>console</title>"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn the_audit_trail_on_the_dashboard_hides_whole_client_ids_too() {
    let harness = harness(open_api(), access_required(), None).await;
    call(
        &harness.router,
        token_request(
            json!({"client_id": "1100000001", "totp": "000000"}),
            "203.0.113.8",
        ),
    )
    .await;
    let request = Request::get("/api/state")
        .header(JWT_HEADER, harness.signed())
        .body(Body::empty())
        .unwrap();
    let (_, _, state) = call(&harness.router, request).await;
    assert_eq!(state["requests"][0]["client_id"], "••••0001");
    assert!(
        !state.to_string().contains("1100000001"),
        "no route to the browser may carry a whole client ID"
    );
}

#[tokio::test]
async fn rate_limited_requests_cost_no_database_write_and_bad_json_is_audited() {
    let harness = harness(open_api(), access_required(), None).await;
    let garbage = Request::post("/api/v1/token")
        .header("cf-connecting-ip", "192.0.2.50")
        .body(Body::from("{not json"))
        .unwrap();
    assert_eq!(
        call(&harness.router, garbage).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        harness.store.recent_requests(100).await.unwrap().len(),
        1,
        "malformed JSON passes the rate limit and the audit like any request"
    );
    for _ in 0..40 {
        call(&harness.router, token_request(json!({}), "192.0.2.51")).await;
    }
    assert_eq!(
        harness.store.recent_requests(100).await.unwrap().len(),
        31,
        "only the 30 admitted requests are written; the 10 refused at the limiter are not"
    );
}
