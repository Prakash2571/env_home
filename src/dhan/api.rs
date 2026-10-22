use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use reqwest::{Client, Response, Url};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use time::format_description::well_known::Rfc3339;
use time::macros::format_description;
use time::{OffsetDateTime, PrimitiveDateTime, UtcOffset};

use super::{Session, is_valid_token};
use crate::config::Account;

const HTTP_TIMEOUT_SECONDS: u64 = 20;
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_ERROR_CHARS: usize = 120;
pub const EXPIRY_MARGIN_SECONDS: i64 = 60;

#[derive(Deserialize)]
struct ApiError {
    #[serde(rename = "errorCode")]
    code: Option<String>,
    #[serde(rename = "errorType")]
    kind: Option<String>,
    #[serde(rename = "errorMessage")]
    message: Option<String>,
}

#[derive(Deserialize)]
struct ConsentResponse {
    #[serde(rename = "consentAppId")]
    consent_id: String,
    status: String,
    #[serde(rename = "consentAppStatus")]
    consent_status: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    #[serde(rename = "dhanClientId")]
    client_id: String,
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "expiryTime")]
    expiry_time: String,
}

#[derive(Clone)]
pub struct DhanAuth {
    base: String,
    client: Client,
}

impl DhanAuth {
    pub fn new(base: &str) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECONDS))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| anyhow!("could not initialise the Dhan authentication client"))?;
        Ok(Self {
            base: base.trim_end_matches('/').to_owned(),
            client,
        })
    }

    pub async fn generate_consent(&self, account: &Account) -> Result<String> {
        let response: ConsentResponse = decode(
            self.client
                .post(format!("{}/app/generate-consent", self.base))
                .header("app_id", &account.api_key)
                .header("app_secret", &account.api_secret)
                .query(&[("client_id", &account.client_id)])
                .send()
                .await,
        )
        .await?;
        if response.status != "success"
            || response.consent_status != "GENERATED"
            || response.consent_id.trim().is_empty()
        {
            bail!("Dhan did not generate a valid login consent");
        }
        let mut url = Url::parse(&format!("{}/login/consentApp-login", self.base))
            .map_err(|_| anyhow!("invalid Dhan login endpoint"))?;
        url.query_pairs_mut()
            .append_pair("consentAppId", &response.consent_id);
        Ok(url.into())
    }

    pub async fn exchange_token(&self, account: &Account, token_id: &str) -> Result<Session> {
        if token_id.trim().is_empty()
            || token_id.len() > 4096
            || token_id.chars().any(char::is_control)
        {
            bail!("the callback tokenId is missing or invalid");
        }
        let response: TokenResponse = decode(
            self.client
                .get(format!("{}/app/consumeApp-consent", self.base))
                .header("app_id", &account.api_key)
                .header("app_secret", &account.api_secret)
                .query(&[("tokenId", token_id)])
                .send()
                .await,
        )
        .await?;
        validate_session(
            account,
            response,
            OffsetDateTime::now_utc().unix_timestamp(),
        )
    }
}

async fn decode<T: DeserializeOwned>(response: Result<Response, reqwest::Error>) -> Result<T> {
    let mut response = response.map_err(|_| anyhow!("the Dhan authentication request failed"))?;
    let status = response.status();
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow!("could not read the Dhan authentication response"))?
    {
        if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(body.len()) {
            bail!("the Dhan authentication response exceeded the size limit");
        }
        body.extend_from_slice(&chunk);
    }

    if !status.is_success() {
        let code = status.as_u16();
        match dhan_error(&body) {
            Some(detail) => bail!("Dhan authentication returned HTTP {code}; {detail}"),
            None => bail!("Dhan authentication returned HTTP {code}"),
        }
    }
    if let Ok(value) = serde_json::from_slice(&body) {
        return Ok(value);
    }
    match dhan_error(&body) {
        Some(detail) => bail!("Dhan rejected the request; {detail}"),
        None => bail!("Dhan authentication returned an invalid response"),
    }
}

fn dhan_error(body: &[u8]) -> Option<String> {
    let error: ApiError = serde_json::from_slice(body).ok()?;
    let code = clip(error.code.or(error.kind)?)?;
    match error.message.and_then(clip) {
        Some(message) => Some(format!("{code}: {message}")),
        None => Some(code),
    }
}

fn clip(value: String) -> Option<String> {
    let clipped: String = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_ERROR_CHARS)
        .collect();
    (!clipped.is_empty()).then_some(clipped)
}

fn validate_session(account: &Account, response: TokenResponse, now: i64) -> Result<Session> {
    if response.client_id != account.client_id {
        bail!("Dhan authenticated a different client ID");
    }
    if !is_valid_token(&response.access_token) {
        bail!("Dhan returned an invalid access token");
    }
    let expires_at = parse_expiry(&response.expiry_time)?;
    if expires_at <= now.saturating_add(EXPIRY_MARGIN_SECONDS) {
        bail!("Dhan returned an expired or nearly expired session");
    }
    Ok(Session {
        client_id: response.client_id,
        access_token: response.access_token,
        expires_at,
        obtained_at: now,
    })
}

pub fn parse_expiry(value: &str) -> Result<i64> {
    if let Ok(timestamp) = OffsetDateTime::parse(value, &Rfc3339) {
        return Ok(timestamp.unix_timestamp());
    }
    let seconds = format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]");
    let fractional =
        format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond]");
    let timestamp = PrimitiveDateTime::parse(value, seconds)
        .or_else(|_| PrimitiveDateTime::parse(value, fractional))
        .map_err(|_| anyhow!("Dhan returned an invalid expiryTime"))?;
    let ist = UtcOffset::from_hms(5, 30, 0).map_err(|_| anyhow!("invalid IST offset"))?;
    Ok(timestamp.assume_offset(ist).unix_timestamp())
}

#[cfg(test)]
#[path = "../../tests/unit/dhan_api.rs"]
mod tests;
