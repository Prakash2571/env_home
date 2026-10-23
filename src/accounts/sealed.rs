use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use super::Account;
use crate::crypto::Cipher;
use crate::otp::{decode_base32, encode_base32};
use crate::store::StoredAccount;

/// Everything secret about an account, sealed as one value. No `Debug`.
#[derive(Deserialize, Serialize)]
struct Secrets {
    api_key: String,
    api_secret: String,
    account_id: String,
    pin: String,
    dhan_totp_seed: String,
    api_totp_seed: String,
}

/// A row copied to another alias or client ID fails to open.
fn binding(alias: &str, client_id: &str) -> String {
    format!("account:{alias}:{client_id}")
}

pub fn seal(cipher: &Cipher, account: &Account, at: i64) -> Result<StoredAccount> {
    let secrets = Secrets {
        api_key: account.api_key.clone(),
        api_secret: account.api_secret.clone(),
        account_id: account.account_id.clone(),
        pin: account.pin.clone(),
        dhan_totp_seed: encode_base32(&account.dhan_totp_seed),
        api_totp_seed: encode_base32(&account.api_totp_seed),
    };
    let plain = serde_json::to_string(&secrets).context("could not encode the account")?;
    Ok(StoredAccount {
        alias: account.alias.clone(),
        client_id: account.client_id.clone(),
        sealed: cipher.encrypt(&plain, &binding(&account.alias, &account.client_id))?,
        created_at: at,
        updated_at: at,
    })
}

pub fn open(cipher: &Cipher, stored: &StoredAccount) -> Result<Account> {
    let plain = cipher.decrypt(&stored.sealed, &binding(&stored.alias, &stored.client_id))?;
    let secrets: Secrets =
        serde_json::from_str(&plain).context("the stored account is not readable")?;
    Ok(Account {
        alias: stored.alias.clone(),
        client_id: stored.client_id.clone(),
        api_key: secrets.api_key,
        api_secret: secrets.api_secret,
        account_id: secrets.account_id,
        pin: secrets.pin,
        dhan_totp_seed: decode_base32(&secrets.dhan_totp_seed)?,
        api_totp_seed: decode_base32(&secrets.api_totp_seed)?,
    })
}
