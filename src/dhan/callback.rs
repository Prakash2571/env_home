use std::sync::{Arc, Mutex};

use anyhow::{Result, bail};
use reqwest::Url;
use tokio::sync::mpsc;

use super::is_valid_token;

const MAX_CALLBACK_BYTES: usize = 8192;
const MAX_ATTEMPTS: u32 = 20;
const CANDIDATE_QUEUE: usize = 8;

pub fn parse_redirect(value: &str) -> Result<Url> {
    let raw = value.trim();
    if raw.contains(['{', '}', '*']) {
        bail!("must not contain route metacharacters");
    }
    let url = Url::parse(raw).map_err(|_| anyhow::anyhow!("is not a URL"))?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("must be http or https");
    }
    if url.host_str().unwrap_or_default().is_empty() {
        bail!("must name a host");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("must not carry credentials");
    }
    if url.fragment().is_some() {
        bail!("must not carry a fragment");
    }
    if url.query_pairs().any(|(key, _)| key == "tokenId") {
        bail!("must not pin tokenId");
    }
    if url.path().is_empty() || url.path() == "/" {
        bail!("needs a path, so the route is its own");
    }
    Ok(url)
}

pub fn route_of(url: &Url) -> String {
    let trimmed = url.path().trim_end_matches('/');
    if trimmed.is_empty() {
        "/".into()
    } else {
        trimmed.to_owned()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Delivery {
    Accepted,
    NoLoginPending,
    Invalid,
    TooManyAttempts,
}

struct Pending {
    alias: String,
    sender: mpsc::Sender<String>,
    attempts: u32,
}

#[derive(Clone)]
pub struct CallbackGate {
    redirect: Url,
    pending: Arc<Mutex<Option<Pending>>>,
}

pub struct GateTicket {
    pending: Arc<Mutex<Option<Pending>>>,
    pub candidates: mpsc::Receiver<String>,
}

impl Drop for GateTicket {
    fn drop(&mut self) {
        if let Ok(mut pending) = self.pending.lock() {
            *pending = None;
        }
    }
}

impl CallbackGate {
    pub fn new(redirect: Url) -> Self {
        Self {
            redirect,
            pending: Arc::new(Mutex::new(None)),
        }
    }

    pub fn redirect(&self) -> &Url {
        &self.redirect
    }

    pub fn open(&self, alias: &str) -> GateTicket {
        let (sender, candidates) = mpsc::channel(CANDIDATE_QUEUE);
        if let Ok(mut pending) = self.pending.lock() {
            *pending = Some(Pending {
                alias: alias.to_owned(),
                sender,
                attempts: 0,
            });
        }
        GateTicket {
            pending: Arc::clone(&self.pending),
            candidates,
        }
    }

    pub fn waiting_for(&self) -> Option<String> {
        self.pending
            .lock()
            .ok()?
            .as_ref()
            .map(|pending| pending.alias.clone())
    }

    pub fn deliver(&self, query: &str) -> Delivery {
        let Ok(mut guard) = self.pending.lock() else {
            return Delivery::NoLoginPending;
        };
        let Some(pending) = guard.as_mut() else {
            return Delivery::NoLoginPending;
        };
        if pending.attempts >= MAX_ATTEMPTS {
            return Delivery::TooManyAttempts;
        }
        pending.attempts += 1;
        match callback_token(query, &self.redirect) {
            Ok(token_id) => match pending.sender.try_send(token_id) {
                Ok(()) => Delivery::Accepted,
                Err(_) => Delivery::TooManyAttempts,
            },
            Err(_) => Delivery::Invalid,
        }
    }
}

pub fn callback_token(query: &str, redirect: &Url) -> Result<String> {
    let prefix = redirect.as_str().split('?').next().unwrap_or_default();
    if prefix.len() + 1 + query.len() > MAX_CALLBACK_BYTES {
        bail!("Callback URL is too long");
    }
    let mut actual = redirect.clone();
    actual.set_query(Some(query));

    let tokens: Vec<_> = actual
        .query_pairs()
        .filter(|(key, _)| key == "tokenId")
        .collect();
    if tokens.len() != 1 || !is_valid_token(&tokens[0].1) {
        bail!("Callback must contain exactly one nonempty tokenId");
    }

    let supplied: Vec<_> = actual
        .query_pairs()
        .filter(|(key, _)| key != "tokenId")
        .collect();
    let configured: Vec<_> = redirect.query_pairs().collect();
    let count = |list: &[(std::borrow::Cow<'_, str>, std::borrow::Cow<'_, str>)],
                 pair: &(std::borrow::Cow<'_, str>, std::borrow::Cow<'_, str>)| {
        list.iter().filter(|other| *other == pair).count()
    };
    if supplied.len() != configured.len()
        || configured
            .iter()
            .any(|pair| count(&supplied, pair) != count(&configured, pair))
    {
        bail!("Callback query does not match DHAN_REDIRECT_URL");
    }
    Ok(tokens[0].1.to_string())
}

#[cfg(test)]
#[path = "../../tests/unit/dhan_callback.rs"]
mod tests;
