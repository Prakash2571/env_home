use super::*;
use crate::clock::LoginWindow;
use crate::config::{AdminPolicy, ApiPolicy, TelegramConfig};
use crate::dhan::callback::parse_redirect;
use crate::test_support::{KEY, account, fake_dhan, serve};

async fn config(telegram: Option<TelegramConfig>) -> Config {
    Config {
        http_addr: "127.0.0.1:0".parse().unwrap(),
        home: ".".into(),
        store: StoreConfig::Memory,
        encryption_key: KEY.into(),
        accounts: vec![account("main", "1100000001")],
        redirect_url: parse_redirect("https://login.example.in/dhan/callback/").unwrap(),
        sidecar_url: Some("http://127.0.0.1:1/login".into()),
        dhan_auth_base: fake_dhan("1100000001").await,
        telegram,
        window: LoginWindow::parse("23:58-23:59", "sun").unwrap(),
        api: ApiPolicy {
            require_access_jwt: false,
            client_ip_header: None,
        },
        admin: AdminPolicy {
            require_access_identity: true,
            allowed_emails: Vec::new(),
        },
    }
}

#[tokio::test]
async fn a_complete_config_builds_a_running_service_and_announces_it() {
    let telegram_base = serve(axum::Router::new().route(
        "/{bot}/sendMessage",
        axum::routing::post(|| async { axum::Json(serde_json::json!({"ok": true})) }),
    ))
    .await;
    let telegram = TelegramConfig {
        bot_token: "1:x".into(),
        chat_id: "-1".into(),
        api_base: telegram_base,
    };
    let context = build(config(Some(telegram)).await, 42).await.unwrap();
    let running = context.running.as_ref().unwrap();
    assert_eq!(context.started_at, 42);
    assert_eq!(running.callback_route, "/dhan/callback");
    assert_eq!(running.window_label, "23:58-23:59 IST");
    assert!(running.engine.automation_available());
    assert!(
        running
            .engine
            .live()
            .events()
            .iter()
            .any(|event| event.message.contains("service started with main"))
    );
    for _ in 0..100 {
        if running.engine.notifier().view().delivered > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(
        running.engine.notifier().view().delivered,
        1,
        "the start-up notice reaches Telegram"
    );
}

#[tokio::test]
async fn an_unreachable_postgres_does_not_stop_the_service_from_starting() {
    let mut config = config(None).await;
    config.store =
        StoreConfig::Postgres("host=127.0.0.1 port=1 user=x dbname=x connect_timeout=1".into());
    config.sidecar_url = None;
    let context = build(config, 0).await.unwrap();
    let snapshot = crate::api::snapshot(&context).await;
    assert!(snapshot.configured);
    assert!(!snapshot.store_reachable);
    assert!(!snapshot.automation);
}

#[test]
fn an_unconfigured_context_carries_only_the_problems() {
    let context = unconfigured(
        vec!["X is missing".into()],
        9,
        AdminPolicy {
            require_access_identity: true,
            allowed_emails: Vec::new(),
        },
    );
    assert!(context.running.is_none());
    assert_eq!(context.problems, ["X is missing"]);
}
