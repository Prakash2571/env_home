use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::http::StatusCode;
use axum::routing::post;
use serde_json::{Value, json};

use super::*;
use crate::config::TelegramConfig;
use crate::test_support::serve;

type Bodies = Arc<Mutex<Vec<Value>>>;

async fn fake_telegram(replies: Vec<(StatusCode, Value)>) -> (TelegramConfig, Bodies) {
    let bodies: Bodies = Arc::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&bodies);
    let router = Router::new().route(
        "/{bot}/sendMessage",
        post(move |axum::Json(body): axum::Json<Value>| {
            let index = calls.fetch_add(1, Ordering::SeqCst);
            seen.lock().unwrap().push(body);
            let (status, reply) = replies
                .get(index)
                .cloned()
                .unwrap_or((StatusCode::OK, json!({"ok": true})));
            async move { (status, axum::Json(reply)) }
        }),
    );
    let base = serve(router).await;
    let config = TelegramConfig {
        bot_token: "123:SECRET".into(),
        chat_id: "-100".into(),
        api_base: base,
    };
    (config, bodies)
}

#[test]
fn a_notice_escapes_every_value_and_marks_problems() {
    let html = Notice::problem(Topic::Login, "login <failed>")
        .fact("account", "main & co")
        .error("HTTP 401 <body>")
        .next("retry")
        .html_at(1_792_348_200 + 8 * 3600 + 5 * 60);
    assert!(html.starts_with("🚨 <b>LOGIN · PROBLEM</b> · <i>08:05:00 IST</i>"));
    assert!(html.contains("login &lt;failed&gt;"));
    assert!(html.contains("main &amp; co"));
    assert!(html.contains("<blockquote expandable>HTTP 401 &lt;body&gt;</blockquote>"));
    assert!(html.contains("➡️ <i>retry</i>"));

    let info = Notice::info(Topic::Api, "served").html_at(0);
    assert!(info.starts_with("🛡 <b>TOKEN API</b>"));
    assert!(
        Notice::info(Topic::System, "up")
            .html_at(0)
            .starts_with("🖥 <b>SERVICE</b>")
    );
    assert_eq!(escape("<a&b>"), "&lt;a&amp;b&gt;");
}

#[test]
fn visible_text_strips_markup_and_unescapes() {
    assert_eq!(visible("<b>a &lt;b&gt;</b> &amp; c"), "a <b> & c");
}

#[test]
fn long_text_splits_on_line_breaks_and_numbers_its_parts() {
    let line = "x".repeat(100);
    let text = vec![line.as_str(); 100].join("\n");
    let parts = split(&text);
    assert!(parts.len() > 1);
    assert!(
        parts
            .iter()
            .all(|part| visible(part).chars().count() <= MAX_MESSAGE_CHARS + 20)
    );
    assert!(parts[0].ends_with(&format!("(1 of {})</i>", parts.len())));
    assert_eq!(split("short"), vec!["short".to_owned()]);

    let monster = "y".repeat(MAX_MESSAGE_CHARS * 2);
    assert!(
        split(&monster)
            .iter()
            .all(|part| visible(part).chars().count() <= MAX_MESSAGE_CHARS)
    );
}

#[test]
fn queued_notices_merge_until_the_limit() {
    let mut pending: VecDeque<String> =
        ["a".into(), "b".into(), "x".repeat(MAX_MESSAGE_CHARS)].into();
    assert_eq!(coalesce(&mut pending).as_deref(), Some("a\n\nb"));
    assert_eq!(pending.len(), 1);
    assert!(coalesce(&mut VecDeque::new()).is_none());
}

#[tokio::test]
async fn a_message_is_sent_as_html_to_the_configured_chat() {
    let (config, bodies) = fake_telegram(vec![]).await;
    Channel::new(config)
        .unwrap()
        .send("<b>hello</b>")
        .await
        .unwrap();
    let body = bodies.lock().unwrap()[0].clone();
    assert_eq!(body["chat_id"], "-100");
    assert_eq!(body["parse_mode"], "HTML");
    assert_eq!(body["text"], "<b>hello</b>");
}

#[tokio::test]
async fn a_rate_limit_is_waited_out_and_bad_markup_falls_back_to_plain_text() {
    let (config, bodies) = fake_telegram(vec![
        (
            StatusCode::TOO_MANY_REQUESTS,
            json!({"description": "Too Many Requests", "parameters": {"retry_after": 0}}),
        ),
        (
            StatusCode::BAD_REQUEST,
            json!({"description": "Bad Request: can't parse entities"}),
        ),
    ])
    .await;
    Channel::new(config)
        .unwrap()
        .send("<b>x</b>")
        .await
        .unwrap();
    let bodies = bodies.lock().unwrap().clone();
    assert_eq!(bodies.len(), 3);
    assert!(bodies[2].get("parse_mode").is_none());
    assert_eq!(bodies[2]["text"], "x");
}

#[tokio::test]
async fn a_refusal_is_reported_with_telegrams_reason_and_never_the_token() {
    let (config, _) = fake_telegram(vec![(
        StatusCode::BAD_REQUEST,
        json!({"description": "Bad Request: chat not found"}),
    )])
    .await;
    let error = Channel::new(config)
        .unwrap()
        .send("hi")
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "Telegram returned HTTP 400: Bad Request: chat not found"
    );

    let offline = TelegramConfig {
        bot_token: "123:SECRET".into(),
        chat_id: "1".into(),
        api_base: "http://127.0.0.1:1".into(),
    };
    let error = Channel::new(offline)
        .unwrap()
        .send("hi")
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("could not connect"));
    assert!(
        !error.contains("SECRET"),
        "reqwest errors carry the URL, which carries the bot token"
    );
}

#[tokio::test]
async fn the_notifier_delivers_in_the_background_and_reports_what_it_did() {
    let (config, bodies) = fake_telegram(vec![]).await;
    let notifier = Notifier::start(Channel::new(config).unwrap());
    notifier.send(&Notice::info(Topic::System, "started"));
    for _ in 0..50 {
        if notifier.view().delivered > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let view = notifier.view();
    assert!(view.configured);
    assert_eq!(view.delivered, 1);
    assert!(view.last_delivered_at.is_some());
    assert!(
        bodies.lock().unwrap()[0]["text"]
            .as_str()
            .unwrap()
            .contains("started")
    );
}

#[test]
fn a_disabled_notifier_accepts_and_drops_silently() {
    let notifier = Notifier::disabled();
    notifier.send(&Notice::info(Topic::System, "nobody hears this"));
    assert!(!notifier.view().configured);
    assert_eq!(notifier.view().delivered, 0);
}
