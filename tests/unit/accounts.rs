use super::*;
use crate::crypto::Cipher;
use crate::state::{EventKind, LiveState};
use crate::store::{MemoryStore, Store, StoredToken};
use crate::test_support::{KEY, account};

const OTHER_KEY: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

fn form(alias: &str, client_id: &str) -> AccountForm {
    AccountForm {
        alias: alias.into(),
        client_id: client_id.into(),
        api_key: format!("key-{alias}"),
        api_secret: format!("secret-{alias}"),
        account_id: "9999999999".into(),
        pin: "123456".into(),
        dhan_totp_secret: crate::test_support::DHAN_SEED.into(),
    }
}

fn book_over(store: &Store, key: &str) -> AccountBook {
    AccountBook::new(
        Registry::default(),
        store.clone(),
        Cipher::from_hex(key).unwrap(),
        LiveState::new(&[]),
    )
}

async fn loaded_book() -> (AccountBook, Store) {
    let store = Store::Memory(MemoryStore::default());
    let book = book_over(&store, KEY);
    book.load().await.unwrap();
    (book, store)
}

#[test]
fn a_sealed_account_opens_whole_and_never_shows_a_secret() {
    let cipher = Cipher::from_hex(KEY).unwrap();
    let original = account("main", "1100000001");
    let stored = seal(&cipher, &original, 1_800_000_000).unwrap();
    for secret in ["key", "secret", "123456", crate::test_support::DHAN_SEED] {
        assert!(
            !stored.sealed.contains(secret),
            "{secret} must not be readable"
        );
    }
    let opened = open(&cipher, &stored).unwrap();
    assert_eq!(opened.api_key, original.api_key);
    assert_eq!(opened.api_secret, original.api_secret);
    assert_eq!(opened.account_id, original.account_id);
    assert_eq!(opened.pin, original.pin);
    assert_eq!(opened.dhan_totp_seed, original.dhan_totp_seed);
    assert_eq!(opened.api_totp_seed, original.api_totp_seed);
}

#[test]
fn a_row_moved_to_another_alias_or_client_does_not_open() {
    let cipher = Cipher::from_hex(KEY).unwrap();
    let stored = seal(&cipher, &account("main", "1100000001"), 1_800_000_000).unwrap();
    let moved = StoredAccount {
        alias: "alt".into(),
        ..stored.clone()
    };
    assert!(open(&cipher, &moved).is_err());
    let rebound = StoredAccount {
        client_id: "1100000002".into(),
        ..stored
    };
    assert!(open(&cipher, &rebound).is_err());
}

#[tokio::test]
async fn an_added_account_is_live_stored_encrypted_and_its_seed_shown_once() {
    let (book, store) = loaded_book().await;
    let added = book
        .add(&form("main", "1100000001"), Some("me@example.com"))
        .await
        .unwrap();

    assert_eq!(added.alias, "main");
    let seed = crate::otp::decode_base32(&added.token_api_seed).unwrap();
    assert_eq!(seed.len(), crate::otp::SEED_BYTES);
    assert!(added.otpauth_uri.contains(&added.token_api_seed));

    let live = book.registry().get("main").expect("registered");
    assert_eq!(live.api_totp_seed, seed);
    assert_eq!(live.api_key, "key-main");
    assert!(
        book.live().account("main").is_some(),
        "the dashboard shows it"
    );
    assert!(
        book.live().events().iter().any(
            |event| event.kind == EventKind::System && event.message.contains("me@example.com")
        )
    );

    let rows = store.list_accounts().await.unwrap();
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].sealed.contains("key-main"));
    assert!(!rows[0].sealed.contains(&added.token_api_seed));
}

#[tokio::test]
async fn an_alias_or_client_id_can_be_used_only_once() {
    let (book, _) = loaded_book().await;
    book.add(&form("main", "1100000001"), None).await.unwrap();
    assert!(matches!(
        book.add(&form("main", "1100000009"), None).await,
        Err(AccountError::DuplicateAlias)
    ));
    assert!(matches!(
        book.add(&form("alt", "1100000001"), None).await,
        Err(AccountError::DuplicateClientId)
    ));
    assert!(matches!(
        book.add(&AccountForm { pin: "x".into(), ..form("bad", "1") }, None).await,
        Err(AccountError::Invalid(problems)) if problems[0].field == "pin"
    ));
}

#[tokio::test]
async fn a_restart_loads_every_account_in_the_order_it_was_added() {
    let (book, store) = loaded_book().await;
    let first = book.add(&form("main", "1100000001"), None).await.unwrap();
    book.add(&form("alt", "1100000002"), None).await.unwrap();

    let restarted = book_over(&store, KEY);
    assert!(!restarted.registry().is_loaded());
    assert_eq!(restarted.load().await.unwrap(), 2);
    let aliases: Vec<String> = restarted
        .registry()
        .list()
        .into_iter()
        .map(|account| account.alias)
        .collect();
    assert_eq!(aliases, vec!["main", "alt"]);
    assert_eq!(
        restarted.registry().get("main").unwrap().api_totp_seed,
        crate::otp::decode_base32(&first.token_api_seed).unwrap()
    );
    assert_eq!(restarted.live().accounts().len(), 2);
}

#[tokio::test]
async fn a_row_sealed_under_another_key_is_skipped_and_reported() {
    let store = Store::Memory(MemoryStore::default());
    let old = book_over(&store, OTHER_KEY);
    old.load().await.unwrap();
    old.add(&form("main", "1100000001"), None).await.unwrap();

    let book = book_over(&store, KEY);
    assert_eq!(book.load().await.unwrap(), 0);
    assert!(book.registry().is_loaded());
    assert!(
        book.live()
            .events()
            .iter()
            .any(|event| event.kind == EventKind::Problem && event.message.contains("main"))
    );

    assert!(
        matches!(
            book.add(&form("main", "1100000009"), None).await,
            Err(AccountError::DuplicateAlias)
        ),
        "a skipped row is never overwritten by a new account"
    );
    assert!(matches!(
        book.add(&form("alt", "1100000001"), None).await,
        Err(AccountError::DuplicateClientId)
    ));
    let again = book_over(&store, OTHER_KEY);
    assert_eq!(
        again.load().await.unwrap(),
        1,
        "the row still opens with its own key"
    );
}

#[tokio::test]
async fn removing_an_account_drops_its_secrets_and_tokens_but_not_while_it_logs_in() {
    let (book, store) = loaded_book().await;
    book.add(&form("main", "1100000001"), None).await.unwrap();
    store
        .save_token(&StoredToken {
            alias: "main".into(),
            client_id: "1100000001".into(),
            sealed: "v1.x.y".into(),
            obtained_at: 1_800_000_000,
            expires_at: 1_800_086_000,
            drive: "scheduled".into(),
        })
        .await
        .unwrap();

    let lease = book.registry().begin_login("main").unwrap();
    assert!(matches!(
        book.remove("main", None).await,
        Err(AccountError::Busy)
    ));
    assert!(
        book.registry().get("main").is_some(),
        "a refused removal changes nothing"
    );
    drop(lease);

    let removed = book.remove("main", Some("me@example.com")).await.unwrap();
    assert_eq!(removed.alias, "main");
    assert!(book.registry().get("main").is_none());
    assert!(book.live().account("main").is_none());
    assert!(store.list_accounts().await.unwrap().is_empty());
    assert_eq!(store.latest_token("main").await.unwrap(), None);
    assert!(matches!(
        book.remove("main", None).await,
        Err(AccountError::NotFound)
    ));
}

#[tokio::test]
async fn a_new_token_api_seed_replaces_the_old_one_everywhere() {
    let (book, store) = loaded_book().await;
    let first = book.add(&form("main", "1100000001"), None).await.unwrap();
    let rotated = book.rotate_token_seed("main", None).await.unwrap();
    assert_ne!(rotated.token_api_seed, first.token_api_seed);

    let fresh = crate::otp::decode_base32(&rotated.token_api_seed).unwrap();
    assert_eq!(book.registry().get("main").unwrap().api_totp_seed, fresh);
    let reloaded = book_over(&store, KEY);
    reloaded.load().await.unwrap();
    assert_eq!(
        reloaded.registry().get("main").unwrap().api_totp_seed,
        fresh
    );
    assert!(matches!(
        book.rotate_token_seed("nobody", None).await,
        Err(AccountError::NotFound)
    ));
}

#[tokio::test]
async fn nothing_changes_before_the_stored_accounts_are_loaded() {
    let store = Store::Memory(MemoryStore::default());
    let book = book_over(&store, KEY);
    assert!(matches!(
        book.add(&form("main", "1100000001"), None).await,
        Err(AccountError::NotLoaded)
    ));
    assert!(matches!(
        book.remove("main", None).await,
        Err(AccountError::NotLoaded)
    ));
    assert!(store.list_accounts().await.unwrap().is_empty());
}

#[test]
fn the_registry_finds_accounts_by_alias_and_client_id() {
    let registry = Registry::default();
    registry.replace_all(vec![
        account("main", "1100000001"),
        account("alt", "1100000002"),
    ]);
    assert_eq!(registry.by_client_id("1100000002").unwrap().alias, "alt");
    assert!(registry.by_client_id("").is_none());
    assert!(registry.get("nobody").is_none());
    assert_eq!(registry.list().len(), 2);
}

#[test]
fn debug_output_names_the_account_and_hides_every_secret() {
    let printed = format!("{:?}", account("main", "1100000001"));
    assert!(printed.contains("main") && printed.contains("••••0001"));
    for secret in ["key", "secret", "123456", "9999999999", "1100000001"] {
        assert!(
            !printed.contains(&format!("\"{secret}\"")),
            "{secret} leaked: {printed}"
        );
    }
}

#[test]
fn a_login_checks_out_the_current_account_and_holds_it_until_it_ends() {
    let registry = Registry::default();
    registry.replace_all(vec![account("main", "1100000001")]);
    let (checked_out, lease) = registry.begin_login("main").unwrap();
    assert_eq!(checked_out.alias, "main");
    assert!(matches!(
        registry.take_if_idle("main"),
        Err(AccountError::Busy)
    ));
    drop(lease);
    assert_eq!(registry.take_if_idle("main").unwrap().alias, "main");
    assert!(
        registry.begin_login("main").is_none(),
        "a login queued behind a removal does not run"
    );
    assert!(matches!(
        registry.take_if_idle("main"),
        Err(AccountError::NotFound)
    ));
}

#[test]
fn the_seed_shown_once_never_appears_in_debug_output() {
    let added = Added {
        alias: "main".into(),
        token_api_seed: "SEEDSEEDSEED".into(),
        otpauth_uri: "otpauth://totp/x?secret=SEEDSEEDSEED".into(),
    };
    let printed = format!("{added:?}");
    assert!(
        printed.contains("main") && !printed.contains("SEEDSEEDSEED"),
        "{printed}"
    );
}
