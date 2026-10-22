use axum::Router;
use tokio::net::TcpListener;

use crate::config::Account;
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
        api_seed_shared: false,
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
    LoginEngine::new(EngineParts {
        live: crate::state::LiveState::new(&accounts),
        accounts,
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
