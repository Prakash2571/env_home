//! The API test harness: a running service on the memory store, a local
//! Cloudflare Access team that signs tokens, and request helpers.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

use crate::access::AccessVerifier;
use crate::api::{AppContext, Running, guard, router};
use crate::config::{AccessCheck, AdminPolicy, ApiPolicy};
use crate::dhan::Session;
use crate::login::{LoginEngine, Scheduler};
use crate::otp::{code_at, now_seconds};
use crate::store::{MemoryStore, Store};
use crate::test_support::{
    ACCESS_API_AUD, ACCESS_AUD, AccessSigner, access_team, account, engine, fake_dhan,
};

pub const EMAIL: &str = "operator@example.in";
pub const MONDAY_0900_IST: i64 = 1_792_348_200 + 9 * 3600;

/// A local Cloudflare Access team: it signs tokens and serves the certs.
pub struct TestAccess {
    pub signer: AccessSigner,
    pub team: String,
    pub verifier: AccessVerifier,
}

impl TestAccess {
    pub async fn start() -> Self {
        let signer = AccessSigner::new("test-key");
        let (team, _) = access_team(&signer).await;
        let verifier = AccessVerifier::new(&team).unwrap();
        Self {
            signer,
            team,
            verifier,
        }
    }

    pub fn check(&self, audience: &str) -> Option<AccessCheck> {
        Some(AccessCheck {
            verifier: self.verifier.clone(),
            audience: audience.into(),
        })
    }

    /// Gives a policy that asks for Access the check it needs.
    pub fn arm(&self, admin: AdminPolicy, api: ApiPolicy) -> (AdminPolicy, ApiPolicy) {
        (
            AdminPolicy {
                access: admin.access.or_else(|| self.check(ACCESS_AUD)),
                ..admin
            },
            ApiPolicy {
                access: api.access.or_else(|| self.check(ACCESS_API_AUD)),
                ..api
            },
        )
    }

    pub fn person(&self, email: &str) -> String {
        self.signer
            .person(&self.team, ACCESS_AUD, email, crate::clock::now_unix())
    }

    pub fn engine(&self) -> String {
        self.signer
            .service(&self.team, ACCESS_API_AUD, crate::clock::now_unix())
    }
}

pub struct Harness {
    pub router: Router,
    pub engine: Arc<LoginEngine>,
    pub store: Store,
    pub access: TestAccess,
}

impl Harness {
    /// The operator, signed in through Access.
    pub fn signed(&self) -> String {
        self.access.person(EMAIL)
    }
}

pub async fn harness(api: ApiPolicy, admin: AdminPolicy, sidecar: Option<&str>) -> Harness {
    let access = TestAccess::start().await;
    let (admin, api) = access.arm(admin, api);
    let store = Store::Memory(MemoryStore::default());
    let dhan = fake_dhan("1100000001").await;
    let engine = engine(
        vec![account("main", "1100000001")],
        &dhan,
        sidecar,
        store.clone(),
    );
    let window = crate::clock::LoginWindow::parse("08:00-08:30", "mon-fri").unwrap();
    let (scheduler, handle) = Scheduler::new(
        Arc::clone(&engine),
        window.clone(),
        Arc::new(|| MONDAY_0900_IST),
    );
    tokio::spawn(scheduler.run());
    let context = Arc::new(AppContext {
        admin: admin.clone(),
        running: Some(Running {
            book: crate::accounts::AccountBook::new(
                engine.registry().clone(),
                store.clone(),
                crate::crypto::Cipher::from_hex(crate::test_support::KEY).unwrap(),
                engine.live().clone(),
            ),
            engine: Arc::clone(&engine),
            scheduler: handle,
            window_label: window.label(),
            window,
            api,
            admin,
            callback_route: "/dhan/callback".into(),
        }),
        problems: Vec::new(),
        guard: guard::TokenGuard::default(),
        started_at: 0,
    });
    Harness {
        router: router(context, std::path::Path::new("/nonexistent")),
        engine,
        store,
        access,
    }
}

pub fn open_api() -> ApiPolicy {
    ApiPolicy {
        require_access_jwt: false,
        client_ip_header: Some("cf-connecting-ip".into()),
        access: None,
    }
}

pub fn access_required() -> AdminPolicy {
    AdminPolicy {
        require_access_identity: true,
        allowed_emails: Vec::new(),
        access: None,
    }
}

pub async fn call(
    router: &Router,
    request: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()));
    (status, headers, body)
}

pub fn token_request(body: Value, ip: &str) -> Request<Body> {
    Request::post("/api/v1/token")
        .header("content-type", "application/json")
        .header("cf-connecting-ip", ip)
        .body(Body::from(body.to_string()))
        .unwrap()
}

pub fn current_code() -> String {
    code_at(&account("main", "1").api_totp_seed, now_seconds())
}

pub async fn seal_token(harness: &Harness) {
    let now = crate::clock::now_unix();
    let session = Session {
        client_id: "1100000001".into(),
        access_token: "eyJ.live".into(),
        obtained_at: now,
        expires_at: now + 72_000,
    };
    harness
        .engine
        .vault()
        .seal(
            &harness.engine.account("main").unwrap(),
            &session,
            "scheduled",
        )
        .await
        .unwrap();
}
