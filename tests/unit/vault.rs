use super::*;
use crate::store::MemoryStore;
use crate::test_support::{KEY, account};

fn vault() -> Vault {
    Vault::new(
        Store::Memory(MemoryStore::default()),
        Cipher::from_hex(KEY).unwrap(),
    )
}

fn session(obtained_at: i64, lifetime: i64) -> Session {
    Session {
        client_id: "1100000001".into(),
        access_token: "eyJ.live".into(),
        obtained_at,
        expires_at: obtained_at + lifetime,
    }
}

#[tokio::test]
async fn a_sealed_token_opens_only_for_its_own_account_and_only_while_valid() {
    let vault = vault();
    let main = account("main", "1100000001");
    let now = now_unix();
    let stored = vault
        .seal(&main, &session(now, 86_000), "scheduled")
        .await
        .unwrap();
    assert!(
        !stored.sealed.contains("eyJ.live"),
        "the store never sees the plaintext"
    );

    let open = vault.open_valid(&main, now).await.unwrap().unwrap();
    assert_eq!(open.access_token, "eyJ.live");
    assert_eq!(open.expires_at, now + 86_000);
    assert!(vault.obtained_today(&main).await.unwrap());

    assert!(
        vault
            .open_valid(&main, now + 86_000)
            .await
            .unwrap()
            .is_none(),
        "an expired token is never served"
    );
    assert!(
        vault
            .open_valid(&account("main", "1100000009"), now)
            .await
            .unwrap()
            .is_none(),
        "a token minted for another client ID under the same alias is not served"
    );
    assert!(
        vault
            .open_valid(&account("other", "1100000001"), now)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn a_token_from_an_earlier_day_does_not_count_as_todays_login() {
    let vault = vault();
    let main = account("main", "1100000001");
    let yesterday = now_unix() - 86_400;
    vault
        .seal(&main, &session(yesterday, 2 * 86_400), "scheduled")
        .await
        .unwrap();
    assert!(!vault.obtained_today(&main).await.unwrap());
    assert!(
        vault.open_valid(&main, now_unix()).await.unwrap().is_some(),
        "it is still served while valid"
    );
    assert_eq!(
        token_view(&vault.latest(&main).await.unwrap().unwrap()).drive,
        "scheduled"
    );
}

#[tokio::test]
async fn a_token_sealed_under_another_key_fails_loudly_rather_than_serving_garbage() {
    let store = Store::Memory(MemoryStore::default());
    let main = account("main", "1100000001");
    let first = Vault::new(store.clone(), Cipher::from_hex(KEY).unwrap());
    first
        .seal(&main, &session(now_unix(), 86_000), "manual")
        .await
        .unwrap();
    let rotated = Vault::new(store, Cipher::from_hex(&"f".repeat(64)).unwrap());
    assert!(rotated.open_valid(&main, now_unix()).await.is_err());
    assert_eq!(rotated.store().kind(), "memory");
}
