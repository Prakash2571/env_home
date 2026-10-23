mod form;
mod sealed;

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use anyhow::Result;
use serde::Serialize;
use tokio::sync::Mutex;

pub use crate::store::StoredAccount;
pub use form::{AccountForm, FieldProblem, valid_alias, valid_client_id};
pub use sealed::{open, seal};

use crate::clock::now_unix;
use crate::crypto::Cipher;
use crate::otp::{encode_base32, generate_seed, otpauth_uri};
use crate::state::{EventKind, LiveState};
use crate::store::{AccountConflict, Store};

/// A Dhan account the service logs in. Added on the dashboard, kept sealed in
/// the store, and held here in the clear only for the life of the process.
#[derive(Clone)]
pub struct Account {
    pub alias: String,
    pub client_id: String,
    pub api_key: String,
    pub api_secret: String,
    pub account_id: String,
    pub pin: String,
    pub dhan_totp_seed: Vec<u8>,
    pub api_totp_seed: Vec<u8>,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Account")
            .field("alias", &self.alias)
            .field("client_id", &crate::state::client_id_hint(&self.client_id))
            .finish_non_exhaustive()
    }
}

#[derive(Default)]
struct Accounts {
    accounts: Vec<Account>,
    logging_in: HashSet<String>,
}

/// The accounts in force right now, shared by the login engine, the scheduler
/// and the token API, so a change on the dashboard applies without a restart.
#[derive(Clone, Default)]
pub struct Registry {
    inner: Arc<RwLock<Accounts>>,
    loaded: Arc<AtomicBool>,
}

/// Held for the whole of one login. While it lives the account cannot be
/// removed, and a login can only start for an account that is still here.
pub struct LoginLease {
    registry: Registry,
    alias: String,
}

impl Drop for LoginLease {
    fn drop(&mut self) {
        self.registry.write().logging_in.remove(&self.alias);
    }
}

impl Registry {
    fn read(&self) -> RwLockReadGuard<'_, Accounts> {
        self.inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self) -> RwLockWriteGuard<'_, Accounts> {
        self.inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn list(&self) -> Vec<Account> {
        self.read().accounts.clone()
    }

    pub fn get(&self, alias: &str) -> Option<Account> {
        self.read()
            .accounts
            .iter()
            .find(|account| account.alias == alias)
            .cloned()
    }

    pub fn by_client_id(&self, client_id: &str) -> Option<Account> {
        if client_id.is_empty() {
            return None;
        }
        self.read()
            .accounts
            .iter()
            .find(|account| account.client_id == client_id)
            .cloned()
    }

    /// The account as it is now, checked out for a login. None if it was
    /// removed, which a login queued behind another must not outlive.
    pub fn begin_login(&self, alias: &str) -> Option<(Account, LoginLease)> {
        let mut inner = self.write();
        let account = inner
            .accounts
            .iter()
            .find(|account| account.alias == alias)
            .cloned()?;
        inner.logging_in.insert(alias.to_owned());
        Some((
            account,
            LoginLease {
                registry: self.clone(),
                alias: alias.to_owned(),
            },
        ))
    }

    /// Replaces the whole set and marks it loaded.
    pub fn replace_all(&self, accounts: Vec<Account>) {
        self.write().accounts = accounts;
        self.loaded.store(true, Ordering::SeqCst);
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded.load(Ordering::SeqCst)
    }

    fn insert(&self, account: Account) {
        self.write().accounts.push(account);
    }

    fn replace(&self, account: Account) {
        if let Some(slot) = self
            .write()
            .accounts
            .iter_mut()
            .find(|existing| existing.alias == account.alias)
        {
            *slot = account;
        }
    }

    /// Takes the account out unless a login holds it, in one step, so no
    /// login can start between the check and the removal.
    fn take_if_idle(&self, alias: &str) -> Result<Account, AccountError> {
        let mut inner = self.write();
        let index = inner
            .accounts
            .iter()
            .position(|account| account.alias == alias)
            .ok_or(AccountError::NotFound)?;
        if inner.logging_in.contains(alias) {
            return Err(AccountError::Busy);
        }
        Ok(inner.accounts.remove(index))
    }
}

/// What the dashboard shows once, right after an account is added or its
/// token API seed is replaced. The service never returns the seed again.
#[derive(Serialize)]
pub struct Added {
    pub alias: String,
    pub token_api_seed: String,
    pub otpauth_uri: String,
}

impl std::fmt::Debug for Added {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Added")
            .field("alias", &self.alias)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub enum AccountError {
    Invalid(Vec<FieldProblem>),
    DuplicateAlias,
    DuplicateClientId,
    NotFound,
    Busy,
    NotLoaded,
    Storage(anyhow::Error),
}

/// Adds, removes and re-keys accounts: the store first, then the registry and
/// the dashboard, one change at a time.
#[derive(Clone)]
pub struct AccountBook {
    registry: Registry,
    store: Store,
    cipher: Cipher,
    live: LiveState,
    turn: Arc<Mutex<()>>,
}

fn by(who: Option<&str>) -> &str {
    who.unwrap_or("the dashboard")
}

impl AccountBook {
    pub fn new(registry: Registry, store: Store, cipher: Cipher, live: LiveState) -> Self {
        Self {
            registry,
            store,
            cipher,
            live,
            turn: Arc::new(Mutex::new(())),
        }
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    pub fn live(&self) -> &LiveState {
        &self.live
    }

    /// Opens every stored account. A row that will not open (another
    /// encryption key, or edited by hand) is skipped and reported, never served.
    pub async fn load(&self) -> Result<usize> {
        let _turn = self.turn.lock().await;
        let rows = self.store.list_accounts().await?;
        let mut accounts = Vec::with_capacity(rows.len());
        for row in &rows {
            match open(&self.cipher, row) {
                Ok(account) => accounts.push(account),
                Err(error) => {
                    tracing::warn!(alias = %row.alias, %error, "a stored account does not open");
                    self.live.note(
                        EventKind::Problem,
                        Some(&row.alias),
                        format!(
                            "account {} does not open with this CBL_ENCRYPTION_KEY and is skipped",
                            row.alias
                        ),
                    );
                }
            }
        }
        for account in &accounts {
            self.live.add_account(account);
        }
        let count = accounts.len();
        self.registry.replace_all(accounts);
        Ok(count)
    }

    pub async fn add(&self, form: &AccountForm, who: Option<&str>) -> Result<Added, AccountError> {
        let new = form::parse(form).map_err(AccountError::Invalid)?;
        let _turn = self.turn.lock().await;
        if !self.registry.is_loaded() {
            return Err(AccountError::NotLoaded);
        }
        if self.registry.get(&new.alias).is_some() {
            return Err(AccountError::DuplicateAlias);
        }
        if self.registry.by_client_id(&new.client_id).is_some() {
            return Err(AccountError::DuplicateClientId);
        }
        let account = Account {
            alias: new.alias,
            client_id: new.client_id,
            api_key: new.api_key,
            api_secret: new.api_secret,
            account_id: new.account_id,
            pin: new.pin,
            dhan_totp_seed: new.dhan_totp_seed,
            api_totp_seed: generate_seed(),
        };
        let row = seal(&self.cipher, &account, now_unix()).map_err(AccountError::Storage)?;
        match self.store.insert_account(&row).await {
            Ok(None) => {}
            Ok(Some(AccountConflict::Alias)) => return Err(AccountError::DuplicateAlias),
            Ok(Some(AccountConflict::ClientId)) => return Err(AccountError::DuplicateClientId),
            Err(error) => return Err(AccountError::Storage(error)),
        }
        self.registry.insert(account.clone());
        self.live.add_account(&account);
        self.live.note(
            EventKind::System,
            Some(&account.alias),
            format!("account {} added by {}", account.alias, by(who)),
        );
        Ok(reveal(&account))
    }

    pub async fn remove(&self, alias: &str, who: Option<&str>) -> Result<Account, AccountError> {
        let _turn = self.turn.lock().await;
        if !self.registry.is_loaded() {
            return Err(AccountError::NotLoaded);
        }
        let account = self.registry.take_if_idle(alias)?;
        if let Err(error) = self.store.delete_account(alias).await {
            self.registry.insert(account);
            return Err(AccountError::Storage(error));
        }
        self.live.remove_account(alias);
        self.live.note(
            EventKind::System,
            None,
            format!("account {alias} removed by {}", by(who)),
        );
        Ok(account)
    }

    pub async fn rotate_token_seed(
        &self,
        alias: &str,
        who: Option<&str>,
    ) -> Result<Added, AccountError> {
        let _turn = self.turn.lock().await;
        let account = Account {
            api_totp_seed: generate_seed(),
            ..self.loaded(alias)?
        };
        let row = seal(&self.cipher, &account, now_unix()).map_err(AccountError::Storage)?;
        let resealed = self
            .store
            .reseal_account(&row.alias, &row.client_id, &row.sealed, row.updated_at)
            .await
            .map_err(AccountError::Storage)?;
        if !resealed {
            return Err(AccountError::NotFound);
        }
        self.registry.replace(account.clone());
        self.live.note(
            EventKind::System,
            Some(alias),
            format!(
                "token API seed of {alias} replaced by {}; the old one no longer works",
                by(who)
            ),
        );
        Ok(reveal(&account))
    }

    fn loaded(&self, alias: &str) -> Result<Account, AccountError> {
        if !self.registry.is_loaded() {
            return Err(AccountError::NotLoaded);
        }
        self.registry.get(alias).ok_or(AccountError::NotFound)
    }
}

fn reveal(account: &Account) -> Added {
    Added {
        alias: account.alias.clone(),
        token_api_seed: encode_base32(&account.api_totp_seed),
        otpauth_uri: otpauth_uri(&account.alias, &account.api_totp_seed),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/accounts.rs"]
mod tests;
