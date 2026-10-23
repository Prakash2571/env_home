use anyhow::Result;

use crate::accounts::Account;
use crate::clock::{ist_day_number, now_unix};
use crate::crypto::Cipher;
use crate::dhan::Session;
use crate::dhan::api::EXPIRY_MARGIN_SECONDS;
use crate::state::TokenView;
use crate::store::{Store, StoredToken};

#[derive(Clone)]
pub struct Vault {
    store: Store,
    cipher: Cipher,
}

pub struct OpenToken {
    pub access_token: String,
    pub obtained_at: i64,
    pub expires_at: i64,
}

fn binding(account: &Account, obtained_at: i64, expires_at: i64) -> String {
    format!(
        "{}:{}:{obtained_at}:{expires_at}",
        account.alias, account.client_id
    )
}

impl Vault {
    pub fn new(store: Store, cipher: Cipher) -> Self {
        Self { store, cipher }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub async fn seal(
        &self,
        account: &Account,
        session: &Session,
        drive: &str,
    ) -> Result<StoredToken> {
        let token = StoredToken {
            alias: account.alias.clone(),
            client_id: account.client_id.clone(),
            sealed: self.cipher.encrypt(
                &session.access_token,
                &binding(account, session.obtained_at, session.expires_at),
            )?,
            obtained_at: session.obtained_at,
            expires_at: session.expires_at,
            drive: drive.to_owned(),
        };
        self.store.save_token(&token).await?;
        Ok(token)
    }

    pub async fn latest(&self, account: &Account) -> Result<Option<StoredToken>> {
        Ok(self
            .store
            .latest_token(&account.alias)
            .await?
            .filter(|token| token.client_id == account.client_id))
    }

    pub async fn open_valid(&self, account: &Account, now: i64) -> Result<Option<OpenToken>> {
        let Some(token) = self.latest(account).await? else {
            return Ok(None);
        };
        if token.expires_at <= now.saturating_add(EXPIRY_MARGIN_SECONDS) {
            return Ok(None);
        }
        let access_token = self.cipher.decrypt(
            &token.sealed,
            &binding(account, token.obtained_at, token.expires_at),
        )?;
        Ok(Some(OpenToken {
            access_token,
            obtained_at: token.obtained_at,
            expires_at: token.expires_at,
        }))
    }

    pub async fn obtained_today(&self, account: &Account) -> Result<bool> {
        let now = now_unix();
        Ok(self.latest(account).await?.is_some_and(|token| {
            ist_day_number(token.obtained_at) == ist_day_number(now)
                && token.expires_at > now + EXPIRY_MARGIN_SECONDS
        }))
    }
}

pub fn token_view(token: &StoredToken) -> TokenView {
    TokenView {
        obtained_at: token.obtained_at,
        expires_at: token.expires_at,
        drive: token.drive.clone(),
    }
}

#[cfg(test)]
#[path = "../tests/unit/vault.rs"]
mod tests;
