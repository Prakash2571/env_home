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
    let ours: Vec<AttemptRecord> = store
        .recent_attempts(1_000)
        .await
        .unwrap()
        .into_iter()
        .filter(|row| row.alias == alias)
        .collect();
    assert_eq!(
        ours.len(),
        3,
        "a shared database holds other runs' rows; only ours count"
    );
    assert_eq!(ours[0].started_at, base + 20, "newest first");
    assert_eq!(
        store.recent_attempts(2).await.unwrap().len(),
        2,
        "the limit is honoured"
    );

    let mut refused = request(base + 1, "refused");
    refused.client_id.clone_from(&alias);
    let mut served = request(base + 2, "served");
    served.client_id.clone_from(&alias);
    store.record_request(&refused).await.unwrap();
    store.record_request(&served).await.unwrap();
    let requests: Vec<RequestRecord> = store
        .recent_requests(1_000)
        .await
        .unwrap()
        .into_iter()
        .filter(|row| row.client_id == alias)
        .collect();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].outcome, "served");
    assert_eq!(requests[0].ip.as_deref(), Some("203.0.113.7"));
    assert_eq!(store.recent_requests(1).await.unwrap().len(), 1);

    honours_the_account_contract(&store, &alias, base).await;
}

fn stored_account(alias: &str, client_id: &str, at: i64) -> StoredAccount {
    StoredAccount {
        alias: alias.into(),
        client_id: client_id.into(),
        sealed: format!("v1.account.{at}"),
        created_at: at,
        updated_at: at,
    }
}

async fn honours_the_account_contract(store: &Store, alias: &str, base: i64) {
    let first = format!("{alias}a");
    let second = format!("{alias}b");
    let ours = |rows: Vec<StoredAccount>| -> Vec<StoredAccount> {
        rows.into_iter()
            .filter(|row| row.alias.starts_with(alias))
            .collect()
    };

    store
        .save_account(&stored_account(&second, &format!("{base}2"), base))
        .await
        .unwrap();
    store
        .save_account(&stored_account(&first, &format!("{base}1"), base))
        .await
        .unwrap();
    let rows = ours(store.list_accounts().await.unwrap());
    assert_eq!(
        rows.iter()
            .map(|row| row.alias.as_str())
            .collect::<Vec<_>>(),
        [second.as_str(), first.as_str()],
        "listed in the order they were added, not by name"
    );

    let mut resealed = stored_account(&second, &format!("{base}2"), base + 60);
    resealed.sealed = "v1.account.new".into();
    store.save_account(&resealed).await.unwrap();
    let rows = ours(store.list_accounts().await.unwrap());
    assert_eq!(rows[0].sealed, "v1.account.new");
    assert_eq!(
        rows[0].created_at, base,
        "re-sealing keeps when it was added"
    );
    assert_eq!(rows[0].updated_at, base + 60);

    assert!(
        store
            .save_account(&stored_account(
                &format!("{alias}c"),
                &format!("{base}1"),
                base
            ))
            .await
            .is_err(),
        "two accounts may not share a client ID"
    );

    store.save_token(&token(&second, base)).await.unwrap();
    assert!(store.delete_account(&second).await.unwrap());
    assert_eq!(
        store.latest_token(&second).await.unwrap(),
        None,
        "its tokens go too"
    );
    assert!(!store.delete_account(&second).await.unwrap());
    assert!(store.delete_account(&first).await.unwrap());
    assert!(ours(store.list_accounts().await.unwrap()).is_empty());
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

async fn prunes_past_retention(store: Store, base: i64) {
    let alias = format!("prune{base}");
    let now = base + KEEP_HISTORY_SECONDS + 10;
    store.save_token(&token(&alias, base)).await.unwrap();
    store.save_token(&token(&alias, now - 60)).await.unwrap();
    store
        .record_attempt(&attempt(&alias, base, true))
        .await
        .unwrap();
    let mut old = request(base, "served");
    old.client_id.clone_from(&alias);
    store.record_request(&old).await.unwrap();
    assert!(store.prune(now).await.unwrap() >= 3);
    assert_eq!(
        store
            .latest_token(&alias)
            .await
            .unwrap()
            .unwrap()
            .obtained_at,
        now - 60,
        "the current token survives"
    );
    assert!(
        store
            .recent_attempts(10_000)
            .await
            .unwrap()
            .iter()
            .all(|row| row.alias != alias)
    );
    assert!(
        store
            .recent_requests(10_000)
            .await
            .unwrap()
            .iter()
            .all(|row| row.client_id != alias)
    );
}

#[tokio::test]
async fn history_and_old_tokens_are_pruned_but_the_current_token_is_kept() {
    prunes_past_retention(Store::Memory(MemoryStore::default()), 1_700_000_000).await;
    if let Ok(url) = std::env::var("CBL_TEST_DATABASE_URL") {
        prunes_past_retention(
            Store::Postgres(PgStore::new(&url)),
            1_600_000_000 + i64::from(std::process::id() % 1000),
        )
        .await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn services_starting_together_on_an_empty_database_all_migrate_it() {
    let Ok(url) = std::env::var("CBL_TEST_DATABASE_URL") else {
        eprintln!("CBL_TEST_DATABASE_URL is not set; the concurrent migration test is skipped");
        return;
    };
    let schema = format!("migrate_race_{}", std::process::id());
    let (admin, connection) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(connection);
    admin
        .batch_execute(&format!(
            "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}"
        ))
        .await
        .unwrap();

    let scoped = format!("{url} options=-csearch_path={schema}");
    let starts = (0..6).map(|_| {
        let store = Store::Postgres(PgStore::new(&scoped));
        tokio::spawn(async move { store.is_reachable().await })
    });
    let reachable = futures_util::future::join_all(starts).await;

    admin
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
        .await
        .unwrap();
    assert!(
        reachable.into_iter().all(|started| started.unwrap()),
        "every service that starts at once must come up"
    );
}
