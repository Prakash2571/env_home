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
