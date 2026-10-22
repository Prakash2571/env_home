use std::time::Duration;

use anyhow::{Result, anyhow, bail};
use reqwest::Client;
use serde::Deserialize;

use super::notice::visible;
use crate::config::TelegramConfig;

pub const MAX_MESSAGE_CHARS: usize = 3_900;
pub const MAX_SENDS: u32 = 3;
pub const MAX_RETRY_AFTER_SECONDS: u64 = 60;
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Deserialize)]
struct SendError {
    description: Option<String>,
    parameters: Option<RetryParameters>,
}

#[derive(Deserialize)]
struct RetryParameters {
    retry_after: Option<u64>,
}

pub fn split(text: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        let line: String = if visible(line).chars().count() > MAX_MESSAGE_CHARS {
            visible(line).chars().take(MAX_MESSAGE_CHARS).collect()
        } else {
            line.to_owned()
        };
        let candidate_len = visible(&current).chars().count() + visible(&line).chars().count() + 1;
        if !current.is_empty() && candidate_len > MAX_MESSAGE_CHARS {
            parts.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push('\n');
        }
        current.push_str(&line);
    }
    if !current.is_empty() || parts.is_empty() {
        parts.push(current);
    }
    let total = parts.len();
    if total > 1 {
        parts
            .iter()
            .enumerate()
            .map(|(index, part)| format!("{part}\n<i>({} of {total})</i>", index + 1))
            .collect()
    } else {
        parts
    }
}

#[derive(Clone)]
pub struct Channel {
    config: TelegramConfig,
    client: Client,
}

impl Channel {
    pub fn new(config: TelegramConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .map_err(|_| anyhow!("could not build the Telegram client"))?;
        Ok(Self { config, client })
    }

    pub async fn send(&self, text: &str) -> Result<()> {
        for part in split(text) {
            match self.send_one(&part, true).await {
                Err(detail) if detail.contains("can't parse entities") => {
                    tracing::warn!(%detail, "Telegram refused the markup; sending as plain text");
                    self.send_one(&visible(&part), false)
                        .await
                        .map_err(|detail| anyhow!(detail))?;
                }
                Err(detail) => bail!(detail),
                Ok(()) => {}
            }
        }
        Ok(())
    }

    async fn send_one(&self, text: &str, marked_up: bool) -> Result<(), String> {
        let url = format!(
            "{}/bot{}/sendMessage",
            self.config.api_base.trim_end_matches('/'),
            self.config.bot_token
        );
        let mut body = serde_json::json!({
            "chat_id": self.config.chat_id,
            "text": text,
            "link_preview_options": { "is_disabled": true },
        });
        if marked_up {
            body["parse_mode"] = "HTML".into();
        }
        let mut last = String::new();
        for attempt in 1..=MAX_SENDS {
            let response = match self.client.post(&url).json(&body).send().await {
                Ok(response) => response,
                Err(error) => {
                    last = unreachable(&error);
                    continue;
                }
            };
            if response.status().is_success() {
                return Ok(());
            }
            let status = response.status().as_u16();
            let refusal = response.json::<SendError>().await.ok();
            let wait = refusal
                .as_ref()
                .and_then(|body| body.parameters.as_ref())
                .and_then(|params| params.retry_after);
            last = match refusal
                .as_ref()
                .and_then(|body| body.description.as_deref())
            {
                Some(description) => format!("Telegram returned HTTP {status}: {description}"),
                None => format!("Telegram returned HTTP {status} with no description"),
            };
            match wait {
                Some(seconds) if attempt < MAX_SENDS => {
                    tokio::time::sleep(Duration::from_secs(seconds.min(MAX_RETRY_AFTER_SECONDS)))
                        .await;
                }
                _ => return Err(last),
            }
        }
        Err(format!("{last}, after {MAX_SENDS} attempts"))
    }
}

fn unreachable(error: &reqwest::Error) -> String {
    let kind = if error.is_timeout() {
        "timed out"
    } else if error.is_connect() {
        "could not connect"
    } else {
        "failed before a reply"
    };
    format!("the Telegram request {kind}")
}
