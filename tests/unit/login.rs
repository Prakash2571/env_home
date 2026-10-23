use axum::Router;
use axum::http::StatusCode;
use axum::routing::{get, post};
use serde_json::{Value, json};

use super::*;
use crate::clock::LoginWindow;
use crate::crypto::Cipher;
use crate::dhan::callback::parse_redirect;
use crate::login::scheduler::Now;
use crate::state::StageStatus;
use crate::store::{MemoryStore, Store};
use crate::test_support::{KEY, account, serve};

#[derive(Clone)]
struct Fakes {
    consent: (StatusCode, Value),
    exchange_client: String,
    sidecar: Option<Value>,
}

impl Default for Fakes {
    fn default() -> Self {
        Self {
            consent: (
                StatusCode::OK,
                json!({"consentAppId": "c-1", "status": "success", "consentAppStatus": "GENERATED"}),
            ),
            exchange_client: "1100000001".into(),
            sidecar: Some(json!({"ok": true, "tokenId": "tid-1"})),
        }
    }
}

fn expiry() -> String {
    (time::OffsetDateTime::now_utc() + time::Duration::hours(20))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

async fn harness(fakes: Fakes, manual_timeout: Duration) -> (Arc<LoginEngine>, Store) {
    let consent = fakes.consent.clone();
    let client = fakes.exchange_client.clone();
    let dhan = serve(
        Router::new()
            .route("/app/generate-consent", post(move || {
                let consent = consent.clone();
                async move { (consent.0, axum::Json(consent.1)) }
            }))
            .route("/app/consumeApp-consent", get(move || {
                let client = client.clone();
                async move { axum::Json(json!({"dhanClientId": client, "accessToken": "eyJ.live", "expiryTime": expiry()})) }
            })),
    )
    .await;
    let sidecar = match fakes.sidecar {
        Some(reply) => {
            let url = serve(Router::new().route(
                "/login",
                post(move || {
                    let reply = reply.clone();
                    async move { axum::Json(reply) }
                }),
            ))
            .await;
            Some(Sidecar::with_code_lifetime(&format!("{url}/login"), 0).unwrap())
        }
        None => None,
    };
    let accounts = vec![account("main", "1100000001")];
    let store = Store::Memory(MemoryStore::default());
    let live = LiveState::new(&accounts);
    let registry = crate::accounts::Registry::default();
    registry.replace_all(accounts);
    let engine = LoginEngine::new(EngineParts {
        live,
        accounts: registry,
        dhan: DhanAuth::new(&dhan).unwrap(),
        sidecar,
        gate: CallbackGate::new(parse_redirect("https://login.example.in/dhan/callback").unwrap()),
        vault: Vault::new(store.clone(), Cipher::from_hex(KEY).unwrap()),
        notifier: Notifier::disabled(),
        manual_timeout,
    });
    (engine, store)
}

fn statuses(engine: &LoginEngine) -> Vec<StageStatus> {
    engine
        .live()
        .account("main")
        .unwrap()
        .stages
        .iter()
        .map(|stage| stage.status)
        .collect()
}

#[tokio::test]
async fn an_automated_login_runs_every_stage_and_stores_a_token_that_opens() {
    let (engine, store) = harness(Fakes::default(), MANUAL_TIMEOUT).await;
    let session = engine.login("main", Drive::Automated).await.unwrap();
    assert_eq!(session.access_token, "eyJ.live");
    use StageStatus::{Done, Skipped};
    assert_eq!(
        statuses(&engine),
        [Done, Done, Done, Done, Done, Skipped],
        "notified is skipped without Telegram"
    );

    let view = engine.live().account("main").unwrap();
    assert_eq!(view.phase, Phase::Ready);
    assert!(view.token.is_some() && view.consent_url.is_none());
    let open = engine
        .vault()
        .open_valid(&engine.account("main").unwrap(), now_unix())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(open.access_token, "eyJ.live");

    let attempts = store.recent_attempts(5).await.unwrap();
    assert_eq!(attempts[0].outcome, "ok");
    assert!(attempts[0].consent_spent);
    assert_eq!(attempts[0].drive, "automated");
}

#[tokio::test]
async fn a_lockout_from_the_sidecar_stops_the_account_for_the_day_and_keeps_the_consent_link() {
    let fakes = Fakes {
        sidecar: Some(
            json!({"ok": false, "stage": "totp-rejected", "error": "Dhan said: Too many attempts"}),
        ),
        ..Fakes::default()
    };
    let (engine, store) = harness(fakes, MANUAL_TIMEOUT).await;
    let failure = engine.login("main", Drive::Scheduled).await.unwrap_err();
    assert_eq!(failure.stage, StageId::Browser);
    assert!(failure.hopeless);
    let view = engine.live().account("main").unwrap();
    assert_eq!(view.phase, Phase::StoppedForToday);
    assert!(
        view.consent_url.is_some(),
        "a person can still finish from the dashboard"
    );
    assert!(view.last_error.unwrap().contains("Too many attempts"));
    assert_eq!(store.recent_attempts(1).await.unwrap()[0].outcome, "failed");
    assert!(
        engine
            .vault()
            .latest(&engine.account("main").unwrap())
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn without_a_sidecar_an_automated_login_fails_at_the_browser() {
    let (engine, _) = harness(
        Fakes {
            sidecar: None,
            ..Fakes::default()
        },
        MANUAL_TIMEOUT,
    )
    .await;
    assert!(!engine.automation_available());
    let failure = engine.login("main", Drive::Automated).await.unwrap_err();
    assert_eq!(failure.stage, StageId::Browser);
    assert!(failure.detail.contains("manual login"));
}

#[tokio::test]
async fn a_manual_login_publishes_the_consent_link_and_completes_on_the_callback() {
    let (engine, _) = harness(
        Fakes {
            sidecar: None,
            ..Fakes::default()
        },
        MANUAL_TIMEOUT,
    )
    .await;
    let running = Arc::clone(&engine);
    let login = tokio::spawn(async move { running.login("main", Drive::Manual).await });

    for _ in 0..100 {
        if engine.live().account("main").unwrap().consent_url.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let view = engine.live().account("main").unwrap();
    assert_eq!(view.phase, Phase::AwaitingCallback);
    assert!(view.consent_url.unwrap().contains("consentAppId=c-1"));
    assert_eq!(engine.gate().waiting_for().as_deref(), Some("main"));
    assert_eq!(
        engine.gate().deliver("tokenId=manual-1", "203.0.113.9"),
        crate::dhan::callback::Delivery::Accepted
    );

    let session = login.await.unwrap().unwrap();
    assert_eq!(session.client_id, "1100000001");
    assert_eq!(
        engine.gate().waiting_for(),
        None,
        "the gate closes once the login ends"
    );
}

#[tokio::test]
async fn a_manual_login_nobody_finishes_times_out_at_the_callback() {
    let (engine, _) = harness(
        Fakes {
            sidecar: None,
            ..Fakes::default()
        },
        Duration::from_millis(50),
    )
    .await;
    let failure = engine.login("main", Drive::Manual).await.unwrap_err();
    assert_eq!(failure.stage, StageId::Callback);
    assert!(failure.detail.contains("timed out"));
}

#[tokio::test]
async fn the_daily_consent_cap_is_respected_before_dhan_is_called() {
    let (engine, store) = harness(Fakes::default(), MANUAL_TIMEOUT).await;
    let now = now_unix();
    for _ in 0..CONSENTS_PER_DAY {
        store
            .record_attempt(&AttemptRecord {
                alias: "main".into(),
                drive: "scheduled".into(),
                started_at: now,
                finished_at: now,
                outcome: "failed".into(),
                stage: "browser".into(),
                detail: String::new(),
                consent_spent: true,
            })
            .await
            .unwrap();
    }
    let failure = engine.login("main", Drive::Automated).await.unwrap_err();
    assert_eq!(failure.stage, StageId::Consent);
    assert!(failure.hopeless);
}

#[tokio::test]
async fn dhan_refusing_the_consent_or_the_wrong_client_is_reported_at_its_stage() {
    let refused = Fakes {
        consent: (StatusCode::UNAUTHORIZED, json!({"errorCode": "DH-901"})),
        ..Fakes::default()
    };
    let (engine, store) = harness(refused, MANUAL_TIMEOUT).await;
    let failure = engine.login("main", Drive::Automated).await.unwrap_err();
    assert_eq!(failure.stage, StageId::Consent);
    assert!(failure.detail.contains("DH-901"));
    assert!(!store.recent_attempts(1).await.unwrap()[0].consent_spent);

    let wrong = Fakes {
        exchange_client: "1100000999".into(),
        ..Fakes::default()
    };
    let (engine, _) = harness(wrong, MANUAL_TIMEOUT).await;
    let failure = engine.login("main", Drive::Automated).await.unwrap_err();
    assert_eq!(failure.stage, StageId::Exchange);
    assert_eq!(engine.live().account("main").unwrap().phase, Phase::Failed);
}

#[tokio::test]
async fn an_unknown_account_is_refused_and_stored_tokens_show_on_start() {
    let (engine, _) = harness(Fakes::default(), MANUAL_TIMEOUT).await;
    assert!(engine.login("ghost", Drive::Manual).await.is_err());
    engine.login("main", Drive::Automated).await.unwrap();
    engine.live().update("main", |view| {
        view.phase = Phase::Idle;
        view.token = None;
    });
    engine.refresh_token_views().await;
    let view = engine.live().account("main").unwrap();
    assert_eq!(view.phase, Phase::Ready);
    assert!(view.token.is_some());
}

fn fixed(at: i64) -> Now {
    Arc::new(move || at)
}

const MONDAY_0810_IST: i64 = 1_792_348_200 + 8 * 3600 + 10 * 60;

#[tokio::test]
async fn inside_the_window_due_accounts_log_in_and_a_done_account_is_left_alone() {
    let (engine, store) = harness(Fakes::default(), MANUAL_TIMEOUT).await;
    let window = LoginWindow::parse("08:00-08:30", "mon-fri").unwrap();
    let (mut scheduler, _handle) =
        Scheduler::new(Arc::clone(&engine), window, fixed(MONDAY_0810_IST));
    assert_eq!(scheduler.window().label(), "08:00-08:30 IST");

    let wait = scheduler.tick().await;
    assert_eq!(
        wait,
        Duration::from_secs(60),
        "after a pass it retries at the gap"
    );
    assert_eq!(store.recent_attempts(10).await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_hopeless_account_is_not_retried_and_the_summary_names_it_after_the_window() {
    let fakes = Fakes {
        sidecar: Some(json!({"ok": false, "stage": "otp", "error": "Dhan asked for an SMS OTP"})),
        ..Fakes::default()
    };
    let (engine, store) = harness(fakes, MANUAL_TIMEOUT).await;
    let window = LoginWindow::parse("08:00-08:30", "mon-fri").unwrap();
    let (mut scheduler, _handle) =
        Scheduler::new(Arc::clone(&engine), window.clone(), fixed(MONDAY_0810_IST));
    scheduler.tick().await;
    scheduler.tick().await;
    assert_eq!(
        store.recent_attempts(10).await.unwrap().len(),
        1,
        "a hopeless account is tried once"
    );

    let (mut after, _handle) =
        Scheduler::new(Arc::clone(&engine), window, fixed(MONDAY_0810_IST + 3600));
    let wait = after.tick().await;
    assert_eq!(wait, Duration::from_secs(86_400 - 4200));
    let events = engine.live().events();
    assert!(
        events
            .iter()
            .any(|event| event.message.contains("morning summary")
                && event.message.contains("missing main"))
    );
    after.tick().await;
    assert_eq!(
        engine
            .live()
            .events()
            .iter()
            .filter(|event| event.message.contains("morning summary"))
            .count(),
        1
    );
}

#[tokio::test]
async fn a_request_from_the_dashboard_is_served_at_any_hour() {
    let (engine, store) = harness(Fakes::default(), MANUAL_TIMEOUT).await;
    let window = LoginWindow::parse("08:00-08:30", "sat").unwrap();
    let (scheduler, handle) = Scheduler::new(Arc::clone(&engine), window, fixed(MONDAY_0810_IST));
    let running = tokio::spawn(scheduler.run());
    assert!(handle.request(LoginRequest {
        alias: None,
        drive: Drive::Automated
    }));
    for _ in 0..100 {
        if !store.recent_attempts(1).await.unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        store.recent_attempts(1).await.unwrap()[0].drive,
        "automated"
    );
    running.abort();
}

#[test]
fn ist_midnight_is_the_start_of_the_ist_day() {
    assert_eq!(ist_midnight(MONDAY_0810_IST), 1_792_348_200);
}

#[tokio::test]
async fn a_restart_after_the_window_stays_quiet_when_every_account_is_ready() {
    let (engine, _) = harness(Fakes::default(), MANUAL_TIMEOUT).await;
    engine.login("main", Drive::Automated).await.unwrap();
    let window = LoginWindow::parse("00:00-00:01", "mon-sun").unwrap();
    let (mut restarted, _handle) = Scheduler::new(Arc::clone(&engine), window, Arc::new(now_unix));
    restarted.tick().await;
    assert!(
        !engine
            .live()
            .events()
            .iter()
            .any(|event| event.message.contains("morning summary")),
        "a deploy after the window must not repeat 'everyone logged in'"
    );
}
