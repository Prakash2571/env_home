use axum::Router;
use tokio::net::TcpListener;

use crate::accounts::Account;
use crate::otp::decode_base32;

pub const DHAN_SEED: &str = "JBSWY3DPEHPK3PXP";
pub const API_SEED: &str = "GEZDGNBVGY3TQOJQ";
pub const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

pub async fn serve(router: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind a test port");
    let address = listener.local_addr().expect("read the test port");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    format!("http://{address}")
}

pub fn account(alias: &str, client_id: &str) -> Account {
    Account {
        alias: alias.to_owned(),
        client_id: client_id.to_owned(),
        api_key: "key".into(),
        api_secret: "secret".into(),
        account_id: "9999999999".into(),
        pin: "123456".into(),
        dhan_totp_seed: decode_base32(DHAN_SEED).unwrap(),
        api_totp_seed: decode_base32(API_SEED).unwrap(),
    }
}

pub fn future_expiry() -> String {
    (time::OffsetDateTime::now_utc() + time::Duration::hours(20))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

pub async fn fake_dhan(client_id: &str) -> String {
    use axum::routing::{get, post};
    let client_id = client_id.to_owned();
    serve(
        Router::new()
            .route(
                "/app/generate-consent",
                post(|| async {
                    axum::Json(serde_json::json!({
                        "consentAppId": "c-1", "status": "success", "consentAppStatus": "GENERATED"
                    }))
                }),
            )
            .route(
                "/app/consumeApp-consent",
                get(move || {
                    let client_id = client_id.clone();
                    async move {
                        axum::Json(serde_json::json!({
                            "dhanClientId": client_id, "accessToken": "eyJ.live", "expiryTime": future_expiry()
                        }))
                    }
                }),
            ),
    )
    .await
}

pub async fn fake_sidecar(reply: serde_json::Value) -> String {
    use axum::routing::post;
    let url = serve(Router::new().route(
        "/login",
        post(move || {
            let reply = reply.clone();
            async move { axum::Json(reply) }
        }),
    ))
    .await;
    format!("{url}/login")
}

pub fn engine(
    accounts: Vec<Account>,
    dhan_base: &str,
    sidecar_url: Option<&str>,
    store: crate::store::Store,
) -> std::sync::Arc<crate::login::LoginEngine> {
    use crate::login::{EngineParts, LoginEngine};
    let registry = crate::accounts::Registry::default();
    let live = crate::state::LiveState::new(&accounts);
    registry.replace_all(accounts);
    LoginEngine::new(EngineParts {
        live,
        accounts: registry,
        dhan: crate::dhan::api::DhanAuth::new(dhan_base).unwrap(),
        sidecar: sidecar_url
            .map(|url| crate::dhan::sidecar::Sidecar::with_code_lifetime(url, 0).unwrap()),
        gate: crate::dhan::callback::CallbackGate::new(
            crate::dhan::callback::parse_redirect("https://login.example.in/dhan/callback")
                .unwrap(),
        ),
        vault: crate::vault::Vault::new(store, crate::crypto::Cipher::from_hex(KEY).unwrap()),
        notifier: crate::telegram::Notifier::disabled(),
        manual_timeout: std::time::Duration::from_secs(5),
    })
}

/// Signs Cloudflare Access style JWTs with the test-only key in
/// `tests/fixtures/access_test_rsa.pk8`, and serves its public half as certs.
pub struct AccessSigner {
    key: ring::rsa::KeyPair,
    pub kid: String,
}

pub const ACCESS_AUD: &str = "dashboard-aud";
pub const ACCESS_API_AUD: &str = "api-aud";

impl AccessSigner {
    pub fn new(kid: &str) -> Self {
        let der = include_bytes!("../fixtures/access_test_rsa.pk8");
        Self {
            key: ring::rsa::KeyPair::from_pkcs8(der).expect("the fixture key loads"),
            kid: kid.to_owned(),
        }
    }

    pub fn jwks(&self) -> serde_json::Value {
        use base64::Engine;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let public: ring::rsa::PublicKeyComponents<Vec<u8>> = self.key.public().into();
        serde_json::json!({ "keys": [{
            "kid": self.kid, "kty": "RSA", "alg": "RS256", "use": "sig",
            "n": URL_SAFE_NO_PAD.encode(&public.n), "e": URL_SAFE_NO_PAD.encode(&public.e),
        }]})
    }

    pub fn sign_with(&self, header: &serde_json::Value, claims: &serde_json::Value) -> String {
        use base64::Engine;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        let mut signature = vec![0; self.key.public().modulus_len()];
        self.key
            .sign(
                &ring::signature::RSA_PKCS1_SHA256,
                &ring::rand::SystemRandom::new(),
                input.as_bytes(),
                &mut signature,
            )
            .expect("signing works");
        format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature))
    }

    pub fn sign(&self, claims: &serde_json::Value) -> String {
        self.sign_with(
            &serde_json::json!({ "alg": "RS256", "kid": self.kid, "typ": "JWT" }),
            claims,
        )
    }

    /// A person signed in through Access, valid for an hour from `now`.
    pub fn person(&self, issuer: &str, audience: &str, email: &str, now: i64) -> String {
        self.sign(&serde_json::json!({
            "aud": [audience], "iss": issuer, "email": email, "type": "app",
            "iat": now, "nbf": now, "exp": now + 3600, "sub": "user-1",
        }))
    }

    /// A service token (a trading engine), valid for an hour from `now`.
    pub fn service(&self, issuer: &str, audience: &str, now: i64) -> String {
        self.sign(&serde_json::json!({
            "aud": audience, "iss": issuer, "common_name": "engine.access", "type": "app",
            "iat": now, "nbf": now, "exp": now + 3600, "sub": "",
        }))
    }
}

/// A team domain on a local port that serves the signer's certs, counting fetches.
pub async fn access_team(
    signer: &AccessSigner,
) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use axum::routing::get;
    let jwks = signer.jwks();
    let fetches = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = std::sync::Arc::clone(&fetches);
    let team = serve(Router::new().route(
        "/cdn-cgi/access/certs",
        get(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let jwks = jwks.clone();
            async move { axum::Json(jwks) }
        }),
    ))
    .await;
    (team, fetches)
}
