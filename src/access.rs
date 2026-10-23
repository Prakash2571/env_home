//! Verifies the JWT Cloudflare Access puts in `Cf-Access-Jwt-Assertion`, so a
//! request is trusted for what Cloudflare signed, not for a header anyone on
//! the box could set. RS256 against the team's published certs, the
//! application's AUD tag, the team as issuer, and the token's lifetime.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use tokio::sync::RwLock;

pub const JWT_HEADER: &str = "cf-access-jwt-assertion";
/// Clock skew allowed between Cloudflare and this box.
pub const LEEWAY_SECONDS: i64 = 60;
/// Cloudflare rotates its signing keys every six weeks; an hour is plenty fresh.
const KEYS_MAX_AGE: Duration = Duration::from_secs(3600);
/// An unknown key id refetches the certs, but never more often than this.
const REFETCH_GAP: Duration = Duration::from_secs(30);
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_TOKEN_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Identity {
    pub email: Option<String>,
    pub common_name: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessError {
    Malformed,
    UnknownKey,
    KeysUnavailable,
    BadSignature,
    WrongIssuer,
    WrongAudience,
    Expired,
    NotYetValid,
}

impl AccessError {
    pub const fn reason(self) -> &'static str {
        match self {
            Self::Malformed => "the Access token is malformed",
            Self::UnknownKey => "the Access token is signed with an unknown key",
            Self::KeysUnavailable => "the Access certs could not be fetched",
            Self::BadSignature => "the Access token signature is wrong",
            Self::WrongIssuer => "the Access token is from another team",
            Self::WrongAudience => "the Access token is for another application",
            Self::Expired => "the Access token has expired",
            Self::NotYetValid => "the Access token is not valid yet",
        }
    }
}

#[derive(Clone)]
struct RsaKey {
    n: Vec<u8>,
    e: Vec<u8>,
}

#[derive(Default)]
struct Keys {
    by_kid: HashMap<String, RsaKey>,
    fetched_at: Option<Instant>,
    tried_at: Option<Instant>,
}

struct Inner {
    issuer: String,
    certs_url: String,
    http: reqwest::Client,
    keys: RwLock<Keys>,
}

/// One per team domain, shared by the dashboard and the token API checks.
#[derive(Clone)]
pub struct AccessVerifier {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for AccessVerifier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccessVerifier")
            .field("issuer", &self.inner.issuer)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct Header {
    alg: String,
    kid: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

impl Audience {
    fn contains(&self, wanted: &str) -> bool {
        match self {
            Self::One(audience) => audience == wanted,
            Self::Many(audiences) => audiences.iter().any(|audience| audience == wanted),
        }
    }
}

#[derive(Deserialize)]
struct Claims {
    aud: Audience,
    iss: String,
    exp: i64,
    nbf: Option<i64>,
    email: Option<String>,
    common_name: Option<String>,
}

#[derive(Deserialize)]
struct Jwk {
    kid: String,
    kty: String,
    n: String,
    e: String,
}

#[derive(Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

fn decode_part<T: for<'de> Deserialize<'de>>(part: &str) -> Result<T, AccessError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(part)
        .map_err(|_| AccessError::Malformed)?;
    serde_json::from_slice(&bytes).map_err(|_| AccessError::Malformed)
}

impl AccessVerifier {
    /// `team_domain` is the team's Access origin, for example
    /// `https://<team>.cloudflareaccess.com`; it is also the token issuer.
    pub fn new(team_domain: &str) -> Result<Self> {
        let issuer = team_domain.trim().trim_end_matches('/').to_owned();
        let http = reqwest::Client::builder()
            .timeout(FETCH_TIMEOUT)
            .build()
            .context("could not build the Access certs client")?;
        Ok(Self {
            inner: Arc::new(Inner {
                certs_url: format!("{issuer}/cdn-cgi/access/certs"),
                issuer,
                http,
                keys: RwLock::new(Keys::default()),
            }),
        })
    }

    pub async fn verify(
        &self,
        token: &str,
        audience: &str,
        now: i64,
    ) -> Result<Identity, AccessError> {
        if token.len() > MAX_TOKEN_BYTES {
            return Err(AccessError::Malformed);
        }
        let mut parts = token.split('.');
        let (Some(header), Some(claims), Some(signature), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(AccessError::Malformed);
        };
        let parsed: Header = decode_part(header)?;
        let kid = match (parsed.alg.as_str(), parsed.kid) {
            ("RS256", Some(kid)) => kid,
            _ => return Err(AccessError::Malformed),
        };
        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| AccessError::Malformed)?;

        let key = self.key(&kid).await?;
        let signed = &token[..header.len() + 1 + claims.len()];
        ring::signature::RsaPublicKeyComponents {
            n: &key.n,
            e: &key.e,
        }
        .verify(
            &ring::signature::RSA_PKCS1_2048_8192_SHA256,
            signed.as_bytes(),
            &signature,
        )
        .map_err(|_| AccessError::BadSignature)?;

        let claims: Claims = decode_part(claims)?;
        if claims.iss.trim_end_matches('/') != self.inner.issuer {
            return Err(AccessError::WrongIssuer);
        }
        if !claims.aud.contains(audience) {
            return Err(AccessError::WrongAudience);
        }
        if now > claims.exp.saturating_add(LEEWAY_SECONDS) {
            return Err(AccessError::Expired);
        }
        if claims
            .nbf
            .is_some_and(|nbf| now.saturating_add(LEEWAY_SECONDS) < nbf)
        {
            return Err(AccessError::NotYetValid);
        }
        Ok(Identity {
            email: claims
                .email
                .map(|email| email.trim().to_ascii_lowercase())
                .filter(|email| !email.is_empty()),
            common_name: claims.common_name.filter(|name| !name.is_empty()),
        })
    }

    async fn key(&self, kid: &str) -> Result<RsaKey, AccessError> {
        {
            let keys = self.inner.keys.read().await;
            let fresh = keys
                .fetched_at
                .is_some_and(|at| at.elapsed() < KEYS_MAX_AGE);
            if let Some(key) = keys.by_kid.get(kid).filter(|_| fresh) {
                return Ok(key.clone());
            }
        }
        let mut keys = self.inner.keys.write().await;
        let may_fetch = keys.tried_at.is_none_or(|at| at.elapsed() >= REFETCH_GAP);
        let stale = keys
            .fetched_at
            .is_none_or(|at| at.elapsed() >= KEYS_MAX_AGE);
        if may_fetch && (stale || !keys.by_kid.contains_key(kid)) {
            keys.tried_at = Some(Instant::now());
            match self.fetch().await {
                Ok(fetched) => {
                    keys.by_kid = fetched;
                    keys.fetched_at = keys.tried_at;
                }
                Err(error) => tracing::warn!(%error, "could not fetch the Cloudflare Access certs"),
            }
        }
        match keys.by_kid.get(kid) {
            Some(key) => Ok(key.clone()),
            None if keys.fetched_at.is_none() => Err(AccessError::KeysUnavailable),
            None => Err(AccessError::UnknownKey),
        }
    }

    async fn fetch(&self) -> Result<HashMap<String, RsaKey>> {
        let jwks: Jwks = self
            .inner
            .http
            .get(&self.inner.certs_url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let mut keys = HashMap::new();
        for jwk in jwks.keys.into_iter().filter(|jwk| jwk.kty == "RSA") {
            let (Ok(n), Ok(e)) = (
                URL_SAFE_NO_PAD.decode(&jwk.n),
                URL_SAFE_NO_PAD.decode(&jwk.e),
            ) else {
                continue;
            };
            keys.insert(jwk.kid, RsaKey { n, e });
        }
        anyhow::ensure!(!keys.is_empty(), "the certs carry no RSA key");
        Ok(keys)
    }
}

#[cfg(test)]
#[path = "../tests/unit/access.rs"]
mod tests;
