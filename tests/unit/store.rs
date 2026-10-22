use super::*;

fn token(alias: &str, obtained_at: i64) -> StoredToken {
    StoredToken {
        alias: alias.into(),
        client_id: format!("{alias}-id"),
        sealed: format!("v1.sealed.{obtained_at}"),
        obtained_at,
        expires_at: obtained_at + 86_000,
        drive: "scheduled".into(),
    }
}

fn attempt(alias: &str, started_at: i64, consent_spent: bool) -> AttemptRecord {
    AttemptRecord {
        alias: alias.into(),
        drive: "scheduled".into(),
        started_at,
        finished_at: started_at + 40,
        outcome: "failed".into(),
        stage: "browser".into(),
        detail: "Dhan said: Invalid TOTP".into(),
        consent_spent,
    }
}

fn request(at: i64, outcome: &str) -> RequestRecord {
    RequestRecord {
        at,
        client_id: "1100000001".into(),
        alias: Some("main".into()),
        ip: Some("203.0.113.7".into()),
        outcome: outcome.into(),
        reason: "test".into(),
    }
}

async fn honours_the_store_contract(store: Store, base: i64) {
    let alias = format!("contract{base}");
    assert!(store.is_reachable().await);
    assert_eq!(store.latest_token(&alias).await.unwrap(), None);

    store.save_token(&token(&alias, base)).await.unwrap();
    store.save_token(&token(&alias, base + 100)).await.unwrap();
    store
        .save_token(&token("someone_else", base + 500))
        .await
        .unwrap();
    let latest = store.latest_token(&alias).await.unwrap().unwrap();
    assert_eq!(
        latest.obtained_at,
        base + 100,
        "the newest token for that account wins"
    );
    assert_eq!(latest.expires_at, base + 100 + 86_000);
    assert_eq!(latest.sealed, format!("v1.sealed.{}", base + 100));

    store
        .record_attempt(&attempt(&alias, base - 10, true))
        .await
        .unwrap();
    store
        .record_attempt(&attempt(&alias, base + 10, true))
        .await
        .unwrap();
    store
        .record_attempt(&attempt(&alias, base + 20, false))
        .await
        .unwrap();
    assert_eq!(
        store.consents_since(&alias, base).await.unwrap(),
        1,
        "only spent consents since the cut-off count"
    );
    let attempts = store.recent_attempts(2).await.unwrap();
    assert_eq!(attempts.len(), 2);
    assert_eq!(attempts[0].started_at, base + 20, "newest first");

    store
        .record_request(&request(base + 1, "refused"))
        .await
        .unwrap();
    store
        .record_request(&request(base + 2, "served"))
        .await
        .unwrap();
    let requests = store.recent_requests(1).await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].outcome, "served");
    assert_eq!(requests[0].ip.as_deref(), Some("203.0.113.7"));
}

#[tokio::test]
async fn the_memory_store_honours_the_contract() {
    let store = Store::Memory(MemoryStore::default());
    assert_eq!(store.kind(), "memory");
    honours_the_store_contract(store, 1_800_000_000).await;
}

#[tokio::test]
async fn the_postgres_store_honours_the_contract_when_a_test_database_is_given() {
    let Ok(url) = std::env::var("CBL_TEST_DATABASE_URL") else {
        eprintln!("CBL_TEST_DATABASE_URL is not set; the Postgres contract test is skipped");
        return;
    };
    let store = Store::Postgres(PgStore::new(&url));
    assert_eq!(store.kind(), "postgres");
    let base = 1_800_000_000 + i64::from(std::process::id() % 100_000) * 1_000;
    honours_the_store_contract(store.clone(), base).await;
    honours_the_store_contract(store, base + 500_000).await;
}

#[tokio::test]
async fn an_unreachable_postgres_reports_itself_without_panicking() {
    let store = Store::Postgres(PgStore::new(
        "host=127.0.0.1 port=1 user=x dbname=x connect_timeout=1",
    ));
    assert!(!store.is_reachable().await);
    assert!(store.latest_token("main").await.is_err());
    assert!(
        store
            .save_token(&token("main", 1_800_000_000))
            .await
            .is_err()
    );
}

#[test]
fn the_memory_store_keeps_a_bounded_history() {
    let store = MemoryStore::default();
    for at in 0..5_010 {
        store.record_request(&request(at, "served")).unwrap();
    }
    assert_eq!(store.recent_requests(10_000).len(), 5_000);
    assert_eq!(store.recent_requests(-1).len(), 0);
}

#[test]
fn every_migration_is_embedded_in_order() {
    let versions: Vec<i32> = MIGRATIONS.iter().map(|(version, _)| *version).collect();
    assert_eq!(versions, (1..=MIGRATIONS.len() as i32).collect::<Vec<_>>());
    assert!(
        MIGRATIONS[0]
            .1
            .contains("CREATE TABLE IF NOT EXISTS broker_tokens")
    );
}
