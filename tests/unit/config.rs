use std::collections::HashMap;

use super::*;

pub const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
pub const DHAN_SEED: &str = "JBSWY3DPEHPK3PXP";
pub const API_SEED: &str = "GEZDGNBVGY3TQOJQ";

fn complete() -> HashMap<String, String> {
    [
        ("CBL_ENCRYPTION_KEY", KEY),
        ("POSTGRES_PASSWORD", "pw"),
        ("CBL_ACCOUNTS", "Main"),
        (
            "DHAN_REDIRECT_URL",
            "https://login.example.in/dhan/callback",
        ),
        ("DHAN_MAIN_CLIENT_ID", "1100000001"),
        ("DHAN_MAIN_API_KEY", "key"),
        ("DHAN_MAIN_API_SECRET", "secret"),
        ("DHAN_MAIN_ACCOUNT_ID", "9999999999"),
        ("DHAN_MAIN_ACCOUNT_PIN", "123456"),
        ("DHAN_MAIN_TOTP_SECRET", DHAN_SEED),
        ("TOKEN_API_MAIN_TOTP_SECRET", API_SEED),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect()
}

fn load_from(env: &HashMap<String, String>) -> Result<Config, ConfigGap> {
    load(&|name| env.get(name).cloned())
}

fn problems(env: &HashMap<String, String>) -> Vec<String> {
    load_from(env).unwrap_err().problems
}

#[test]
fn a_complete_environment_loads_with_safe_defaults() {
    let config = load_from(&complete()).unwrap();
    assert_eq!(config.http_addr.to_string(), DEFAULT_HTTP_ADDR);
    assert_eq!(config.accounts.len(), 1);
    let account = &config.accounts[0];
    assert_eq!(account.alias, "main", "aliases are case-folded");
    assert!(!account.api_seed_shared);
    assert_eq!(config.window.label(), "08:00-08:30 IST");
    assert!(config.telegram.is_none(), "Telegram is optional");
    assert!(
        config.admin.require_access_identity,
        "the admin routes default to requiring Access"
    );
    assert!(
        !config.api.require_access_jwt,
        "D4 is open, so the API does not demand the Access JWT by default"
    );
    assert_eq!(
        config.api.client_ip_header.as_deref(),
        Some("cf-connecting-ip")
    );
    assert!(
        matches!(config.store, StoreConfig::Postgres(ref url) if url.contains("dbname=broker_login"))
    );
}

#[test]
fn every_problem_is_reported_at_once_by_name_and_never_by_value() {
    let env = HashMap::new();
    let found = problems(&env);
    assert!(found.iter().any(|p| p.contains("CBL_ENCRYPTION_KEY")));
    assert!(found.iter().any(|p| p.contains("POSTGRES_PASSWORD")));
    assert!(found.iter().any(|p| p.contains("CBL_ACCOUNTS")));
    assert!(found.iter().any(|p| p.contains("DHAN_REDIRECT_URL")));

    let mut leaky = complete();
    leaky.insert(
        "CBL_ENCRYPTION_KEY".into(),
        "not-a-key-but-a-secret-value".into(),
    );
    let found = problems(&leaky);
    assert!(
        found
            .iter()
            .all(|p| !p.contains("not-a-key-but-a-secret-value"))
    );
}

#[test]
fn a_shared_api_seed_serves_accounts_without_their_own() {
    let mut env = complete();
    env.remove("TOKEN_API_MAIN_TOTP_SECRET");
    env.insert("TOKEN_API_TOTP_SECRET".into(), API_SEED.into());
    let config = load_from(&env).unwrap();
    assert!(
        config.accounts[0].api_seed_shared,
        "D3 stays open: one shared seed works too"
    );

    env.remove("TOKEN_API_TOTP_SECRET");
    assert!(
        problems(&env)
            .iter()
            .any(|p| p.contains("TOKEN_API_MAIN_TOTP_SECRET"))
    );
}

#[test]
fn the_api_seed_may_never_equal_the_broker_seed() {
    let mut env = complete();
    env.insert("TOKEN_API_MAIN_TOTP_SECRET".into(), DHAN_SEED.into());
    assert!(
        problems(&env)
            .iter()
            .any(|p| p.contains("must not log in to Dhan"))
    );
}

#[test]
fn aliases_must_be_simple_and_unique_and_client_ids_distinct() {
    let mut env = complete();
    env.insert("CBL_ACCOUNTS".into(), "main,main".into());
    assert!(problems(&env).iter().any(|p| p.contains("twice")));

    env.insert("CBL_ACCOUNTS".into(), "ma-in".into());
    assert!(problems(&env).iter().any(|p| p.contains("letters, digits")));

    let mut two = complete();
    two.insert("CBL_ACCOUNTS".into(), "main,alt".into());
    for (key, value) in complete() {
        if let Some(rest) = key.strip_prefix("DHAN_MAIN_") {
            two.insert(format!("DHAN_ALT_{rest}"), value);
        }
    }
    two.insert("TOKEN_API_ALT_TOTP_SECRET".into(), API_SEED.into());
    assert!(
        problems(&two)
            .iter()
            .any(|p| p.contains("share the client ID"))
    );
    two.insert("DHAN_ALT_CLIENT_ID".into(), "1100000002".into());
    assert_eq!(load_from(&two).unwrap().accounts.len(), 2);
}

#[test]
fn seeds_and_credentials_are_validated() {
    let mut env = complete();
    env.insert("DHAN_MAIN_TOTP_SECRET".into(), "not base32!".into());
    env.insert("DHAN_MAIN_API_KEY".into(), "has space".into());
    let found = problems(&env);
    assert!(
        found
            .iter()
            .any(|p| p.contains("DHAN_MAIN_TOTP_SECRET is not a base32"))
    );
    assert!(
        found
            .iter()
            .any(|p| p.contains("DHAN_MAIN_API_KEY must be printable"))
    );
}

#[test]
fn telegram_needs_both_values_or_neither() {
    let mut env = complete();
    env.insert("TELEGRAM_BOT_TOKEN".into(), "123:abc".into());
    assert!(problems(&env).iter().any(|p| p.contains("TELEGRAM")));
    env.insert("TELEGRAM_CHAT_ID".into(), "-100".into());
    let telegram = load_from(&env).unwrap().telegram.unwrap();
    assert_eq!(telegram.api_base, DEFAULT_TELEGRAM_API);
}

#[test]
fn the_store_can_be_memory_or_any_postgres_url() {
    let mut env = complete();
    env.remove("POSTGRES_PASSWORD");
    env.insert("CBL_STORE".into(), "memory".into());
    assert!(matches!(
        load_from(&env).unwrap().store,
        StoreConfig::Memory
    ));

    env.remove("CBL_STORE");
    env.insert("CBL_DATABASE_URL".into(), "postgres://u:p@db/x".into());
    assert!(
        matches!(load_from(&env).unwrap().store, StoreConfig::Postgres(ref url) if url == "postgres://u:p@db/x")
    );
}

#[test]
fn policies_read_their_flags() {
    let mut env = complete();
    env.insert("CBL_API_REQUIRE_ACCESS_JWT".into(), "yes".into());
    env.insert("CBL_ADMIN_REQUIRE_ACCESS".into(), "off".into());
    env.insert("CBL_ADMIN_EMAILS".into(), "A@x.in, b@y.in,".into());
    env.insert("CBL_CLIENT_IP_HEADER".into(), "none".into());
    let config = load_from(&env).unwrap();
    assert!(config.api.require_access_jwt);
    assert!(!config.admin.require_access_identity);
    assert_eq!(config.admin.allowed_emails, ["a@x.in", "b@y.in"]);
    assert!(config.api.client_ip_header.is_none());
}

#[test]
fn bad_addresses_and_windows_are_problems() {
    let mut env = complete();
    env.insert("CBL_HTTP_ADDR".into(), "nowhere".into());
    env.insert("CBL_LOGIN_WINDOW".into(), "8-9".into());
    env.insert("DHAN_REDIRECT_URL".into(), "ftp://x/y".into());
    let found = problems(&env);
    assert!(found.iter().any(|p| p.contains("CBL_HTTP_ADDR")));
    assert!(found.iter().any(|p| p.contains("CBL_LOGIN_WINDOW")));
    assert!(found.iter().any(|p| p.contains("DHAN_REDIRECT_URL")));
}

#[test]
fn basics_fall_back_when_unset() {
    let empty: HashMap<String, String> = HashMap::new();
    let basics = basics(&|name| empty.get(name).cloned());
    assert_eq!(basics.http_addr.to_string(), DEFAULT_HTTP_ADDR);
    assert_eq!(basics.home, PathBuf::from("."));
}

#[test]
fn debug_output_never_carries_a_secret() {
    let config = load_from(&complete()).unwrap();
    let printed = format!("{config:?}");
    for secret in [
        KEY,
        "pw",
        "secret",
        "123456",
        DHAN_SEED,
        "1100000001",
        "9999999999",
    ] {
        assert!(
            !printed.contains(secret),
            "{secret} leaked into Debug output: {printed}"
        );
    }
    assert!(printed.contains("main") && printed.contains("••••0001"));
    let session = crate::dhan::Session {
        client_id: "1100000001".into(),
        access_token: "eyJ.secret".into(),
        expires_at: 2,
        obtained_at: 1,
    };
    assert!(!format!("{session:?}").contains("eyJ.secret"));
}
