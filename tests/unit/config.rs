use std::collections::HashMap;

use super::*;

pub const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn complete() -> HashMap<String, String> {
    [
        ("CBL_ENCRYPTION_KEY", KEY),
        ("POSTGRES_PASSWORD", "pw"),
        (
            "CBL_ACCESS_TEAM_DOMAIN",
            "https://team.cloudflareaccess.com/",
        ),
        ("CBL_ACCESS_DASHBOARD_AUD", "dashboard-aud"),
        ("CBL_ACCESS_API_AUD", "api-aud"),
        (
            "DHAN_REDIRECT_URL",
            "https://login.example.in/dhan/callback",
        ),
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
fn accounts_come_from_the_dashboard_and_old_account_variables_are_named() {
    let config = load_from(&complete()).unwrap();
    assert!(format!("{config:?}").contains("Config"));
    assert!(retired_variables(&|_| None).is_empty());

    let mut env = complete();
    env.insert("CBL_ACCOUNTS".into(), "main".into());
    env.insert("TOKEN_API_TOTP_SECRET".into(), "GEZDGNBVGY3TQOJQ".into());
    env.insert("DHAN_MAIN_API_KEY".into(), "".into());
    assert!(load_from(&env).is_ok(), "they no longer stop the service");
    assert_eq!(
        retired_variables(&|name| env.get(name).cloned()),
        ["CBL_ACCOUNTS", "TOKEN_API_TOTP_SECRET"]
    );
}

#[test]
fn each_access_check_needs_the_team_and_its_own_aud_tag() {
    let config = load_from(&complete()).unwrap();
    let dashboard = config.admin.access.expect("the dashboard check is built");
    assert_eq!(dashboard.audience, "dashboard-aud");
    assert!(format!("{:?}", dashboard.verifier).contains("https://team.cloudflareaccess.com"));
    assert_eq!(config.api.access.unwrap().audience, "api-aud");

    let mut env = complete();
    env.remove("CBL_ACCESS_DASHBOARD_AUD");
    assert!(
        problems(&env)
            .iter()
            .any(|p| p.contains("CBL_ACCESS_DASHBOARD_AUD"))
    );
    env.insert("CBL_ADMIN_REQUIRE_ACCESS".into(), "false".into());
    assert!(
        load_from(&env).is_ok(),
        "local development may turn the check off"
    );

    let mut env = complete();
    env.remove("CBL_ACCESS_API_AUD");
    assert!(load_from(&env).is_ok(), "D4 off: the API needs no AUD tag");
    env.insert("CBL_API_REQUIRE_ACCESS_JWT".into(), "true".into());
    assert!(
        problems(&env)
            .iter()
            .any(|p| p.contains("CBL_ACCESS_API_AUD"))
    );

    for wrong in [
        "http://team.cloudflareaccess.com",
        "https://team.example.com",
        "https://cloudflareaccess.com",
        "https://team.cloudflareaccess.com/cdn-cgi",
        "https://team.cloudflareaccess.com/?x=1",
        "https://team.cloudflareaccess.com:8443",
        "https://evil.com#.cloudflareaccess.com",
    ] {
        let mut env = complete();
        env.insert("CBL_ACCESS_TEAM_DOMAIN".into(), wrong.into());
        let found = problems(&env);
        assert!(
            found
                .iter()
                .any(|p| p.contains("<team>.cloudflareaccess.com")),
            "{wrong} was accepted: {found:?}"
        );
    }
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
    for secret in [KEY, "password=pw"] {
        assert!(
            !printed.contains(secret),
            "{secret} leaked into Debug output: {printed}"
        );
    }
    let session = crate::dhan::Session {
        client_id: "1100000001".into(),
        access_token: "eyJ.secret".into(),
        expires_at: 2,
        obtained_at: 1,
    };
    assert!(!format!("{session:?}").contains("eyJ.secret"));
}
