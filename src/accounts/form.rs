use serde::{Deserialize, Serialize};

use crate::otp::decode_base32;

pub const ALIAS_MAX: usize = 32;
pub const CLIENT_ID_MAX: usize = 20;
pub const MAX_FIELD: usize = 512;
/// 80 bits: below this a TOTP seed is guessable offline.
const MIN_SEED_BYTES: usize = 10;
const PIN_DIGITS: std::ops::RangeInclusive<usize> = 4..=8;
/// Path segments the account routes already use.
const RESERVED_ALIASES: [&str; 1] = ["login"];

/// What the dashboard sends to add an account. Never logged or echoed back.
#[derive(Deserialize)]
pub struct AccountForm {
    pub alias: String,
    pub client_id: String,
    pub api_key: String,
    pub api_secret: String,
    pub account_id: String,
    pub pin: String,
    pub dhan_totp_secret: String,
}

/// A field the form got wrong, named but never repeated.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FieldProblem {
    pub field: &'static str,
    pub message: &'static str,
}

pub struct NewAccount {
    pub alias: String,
    pub client_id: String,
    pub api_key: String,
    pub api_secret: String,
    pub account_id: String,
    pub pin: String,
    pub dhan_totp_seed: Vec<u8>,
}

impl std::fmt::Debug for NewAccount {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NewAccount")
            .field("alias", &self.alias)
            .finish_non_exhaustive()
    }
}

pub fn valid_alias(alias: &str) -> bool {
    !alias.is_empty()
        && alias.len() <= ALIAS_MAX
        && alias
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        && !RESERVED_ALIASES.contains(&alias)
}

pub fn valid_client_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= CLIENT_ID_MAX
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn printable(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_FIELD
        && value.bytes().all(|byte| byte.is_ascii_graphic())
}

pub fn parse(form: &AccountForm) -> Result<NewAccount, Vec<FieldProblem>> {
    let mut problems = Vec::new();
    let mut check = |field: &'static str, ok: bool, message: &'static str| {
        if !ok {
            problems.push(FieldProblem { field, message });
        }
    };

    let alias = form.alias.trim().to_ascii_lowercase();
    check(
        "alias",
        valid_alias(&alias),
        "use up to 32 lower-case letters, digits or _ (not \"login\")",
    );
    let client_id = form.client_id.trim().to_owned();
    check(
        "client_id",
        valid_client_id(&client_id),
        "the Dhan client ID: up to 20 letters or digits",
    );
    let api_key = form.api_key.trim().to_owned();
    check("api_key", printable(&api_key), "required, without spaces");
    let api_secret = form.api_secret.trim().to_owned();
    check(
        "api_secret",
        printable(&api_secret),
        "required, without spaces",
    );
    let account_id = form.account_id.trim().to_owned();
    check(
        "account_id",
        printable(&account_id),
        "required, without spaces",
    );
    let pin = form.pin.trim().to_owned();
    check(
        "pin",
        PIN_DIGITS.contains(&pin.len()) && pin.bytes().all(|byte| byte.is_ascii_digit()),
        "4 to 8 digits",
    );
    let dhan_totp_seed = decode_base32(&form.dhan_totp_secret)
        .ok()
        .filter(|seed| seed.len() >= MIN_SEED_BYTES);
    check(
        "dhan_totp_secret",
        dhan_totp_seed.is_some(),
        "the base32 seed Dhan showed when you turned on TOTP",
    );

    if !problems.is_empty() {
        return Err(problems);
    }
    Ok(NewAccount {
        alias,
        client_id,
        api_key,
        api_secret,
        account_id,
        pin,
        dhan_totp_seed: dhan_totp_seed.unwrap_or_default(),
    })
}

#[cfg(test)]
#[path = "../../tests/unit/accounts_form.rs"]
mod tests;
