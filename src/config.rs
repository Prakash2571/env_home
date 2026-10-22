use std::net::SocketAddr;
use std::path::PathBuf;

use reqwest::Url;

use crate::clock::LoginWindow;
use crate::otp::decode_base32;

pub const DEFAULT_HTTP_ADDR: &str = "127.0.0.1:47440";
pub const DEFAULT_DHAN_AUTH_BASE: &str = "https://auth.dhan.co";
pub const DEFAULT_TELEGRAM_API: &str = "https://api.telegram.org";
const ALIAS_MAX: usize = 32;

#[derive(Clone, Debug)]
pub struct Account {
    pub alias: String,
    pub client_id: String,
    pub api_key: String,
    pub api_secret: String,
    pub account_id: String,
    pub pin: String,
    pub dhan_totp_seed: Vec<u8>,
    pub api_totp_seed: Vec<u8>,
    pub api_seed_shared: bool,
}

#[derive(Clone, Debug)]
pub enum StoreConfig {
    Memory,
    Postgres(String),
}

#[derive(Clone, Debug)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub chat_id: String,
    pub api_base: String,
}

#[derive(Clone, Debug)]
pub struct ApiPolicy {
    pub require_access_jwt: bool,
    pub client_ip_header: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AdminPolicy {
    pub require_access_identity: bool,
    pub allowed_emails: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub http_addr: SocketAddr,
    pub home: PathBuf,
    pub store: StoreConfig,
    pub encryption_key: String,
    pub accounts: Vec<Account>,
    pub redirect_url: Url,
    pub sidecar_url: Option<String>,
    pub dhan_auth_base: String,
    pub telegram: Option<TelegramConfig>,
    pub window: LoginWindow,
    pub api: ApiPolicy,
    pub admin: AdminPolicy,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConfigGap {
    pub problems: Vec<String>,
}

pub struct Basics {
    pub http_addr: SocketAddr,
    pub home: PathBuf,
}

pub fn basics(read: &dyn Fn(&str) -> Option<String>) -> Basics {
    let http_addr = text(read, "CBL_HTTP_ADDR")
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| {
            DEFAULT_HTTP_ADDR
                .parse()
                .expect("the default address parses")
        });
    let home = text(read, "CBL_HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    Basics { http_addr, home }
}

fn text(read: &dyn Fn(&str) -> Option<String>, name: &str) -> Option<String> {
    read(name)
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn flag(read: &dyn Fn(&str) -> Option<String>, name: &str, default: bool) -> bool {
    match text(read, name).map(|value| value.to_ascii_lowercase()) {
        Some(value) if matches!(value.as_str(), "1" | "true" | "yes" | "on") => true,
        Some(value) if matches!(value.as_str(), "0" | "false" | "no" | "off") => false,
        _ => default,
    }
}

struct Reader<'env> {
    read: &'env dyn Fn(&str) -> Option<String>,
    problems: Vec<String>,
}

impl Reader<'_> {
    fn required(&mut self, name: &str) -> String {
        text(self.read, name).unwrap_or_else(|| {
            self.problems.push(format!("{name} is missing"));
            String::new()
        })
    }

    fn printable(&mut self, name: &str) -> String {
        let value = self.required(name);
        if !value.is_empty() && !value.bytes().all(|byte| byte.is_ascii_graphic()) {
            self.problems
                .push(format!("{name} must be printable ASCII without spaces"));
        }
        value
    }

    fn seed(&mut self, name: &str) -> Vec<u8> {
        let value = self.required(name);
        if value.is_empty() {
            return Vec::new();
        }
        decode_base32(&value).unwrap_or_else(|_| {
            self.problems
                .push(format!("{name} is not a base32 TOTP seed"));
            Vec::new()
        })
    }
}

pub fn load(read: &dyn Fn(&str) -> Option<String>) -> Result<Config, ConfigGap> {
    let mut reader = Reader {
        read,
        problems: Vec::new(),
    };
    let Basics { http_addr, home } = basics(read);
    if let Some(value) = text(read, "CBL_HTTP_ADDR")
        && value.parse::<SocketAddr>().is_err()
    {
        reader
            .problems
            .push("CBL_HTTP_ADDR is not a socket address".into());
    }

    let store = load_store(&mut reader);
    let encryption_key = reader.required("CBL_ENCRYPTION_KEY");
    if !encryption_key.is_empty() && crate::crypto::Cipher::from_hex(&encryption_key).is_err() {
        reader
            .problems
            .push("CBL_ENCRYPTION_KEY must be 64 hex characters".into());
    }

    let accounts = load_accounts(&mut reader);
    let redirect_url = load_redirect(&mut reader);
    let window = load_window(&mut reader);
    let telegram = load_telegram(&mut reader);

    let api = ApiPolicy {
        require_access_jwt: flag(read, "CBL_API_REQUIRE_ACCESS_JWT", false),
        client_ip_header: match text(read, "CBL_CLIENT_IP_HEADER") {
            Some(value) if value.eq_ignore_ascii_case("none") => None,
            Some(value) => Some(value.to_ascii_lowercase()),
            None => Some("cf-connecting-ip".into()),
        },
    };
    let admin = AdminPolicy {
        require_access_identity: flag(read, "CBL_ADMIN_REQUIRE_ACCESS", true),
        allowed_emails: text(read, "CBL_ADMIN_EMAILS")
            .map(|value| {
                value
                    .split(',')
                    .map(|email| email.trim().to_ascii_lowercase())
                    .filter(|email| !email.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
    };

    if !reader.problems.is_empty() {
        return Err(ConfigGap {
            problems: reader.problems,
        });
    }
    Ok(Config {
        http_addr,
        home,
        store,
        encryption_key,
        accounts,
        redirect_url: redirect_url.expect("validated above"),
        sidecar_url: text(read, "CBL_SIDECAR_URL"),
        dhan_auth_base: text(read, "CBL_DHAN_AUTH_BASE")
            .unwrap_or_else(|| DEFAULT_DHAN_AUTH_BASE.into()),
        telegram,
        window: window.expect("validated above"),
        api,
        admin,
    })
}

fn load_store(reader: &mut Reader<'_>) -> StoreConfig {
    let read = reader.read;
    if text(read, "CBL_STORE").is_some_and(|value| value.eq_ignore_ascii_case("memory")) {
        return StoreConfig::Memory;
    }
    if let Some(url) = text(read, "CBL_DATABASE_URL") {
        return StoreConfig::Postgres(url);
    }
    let password = reader.required("POSTGRES_PASSWORD");
    let host = text(read, "CBL_DATABASE_HOST").unwrap_or_else(|| "127.0.0.1".into());
    let port = text(read, "CBL_DATABASE_PORT").unwrap_or_else(|| "5432".into());
    let database = text(read, "POSTGRES_DB").unwrap_or_else(|| "broker_login".into());
    let user = text(read, "POSTGRES_USER").unwrap_or_else(|| "broker_login".into());
    StoreConfig::Postgres(format!(
        "host={host} port={port} dbname={database} user={user} password={password}"
    ))
}

fn valid_alias(alias: &str) -> bool {
    !alias.is_empty()
        && alias.len() <= ALIAS_MAX
        && alias
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn load_accounts(reader: &mut Reader<'_>) -> Vec<Account> {
    let aliases: Vec<String> = text(reader.read, "CBL_ACCOUNTS")
        .unwrap_or_default()
        .split(',')
        .map(|alias| alias.trim().to_ascii_lowercase())
        .filter(|alias| !alias.is_empty())
        .collect();
    if aliases.is_empty() {
        reader.problems.push("CBL_ACCOUNTS names no account".into());
        return Vec::new();
    }

    let shared_seed = text(reader.read, "TOKEN_API_TOTP_SECRET");
    let mut accounts: Vec<Account> = Vec::new();
    for alias in aliases {
        if !valid_alias(&alias) {
            reader.problems.push(format!(
                "CBL_ACCOUNTS alias {alias:?} must be letters, digits or _"
            ));
            continue;
        }
        if accounts.iter().any(|account| account.alias == alias) {
            reader
                .problems
                .push(format!("CBL_ACCOUNTS lists {alias} twice"));
            continue;
        }
        accounts.push(load_account(reader, &alias, shared_seed.as_deref()));
    }

    for (index, account) in accounts.iter().enumerate() {
        if accounts[..index]
            .iter()
            .any(|other| other.client_id == account.client_id && !account.client_id.is_empty())
        {
            reader.problems.push(format!(
                "two accounts share the client ID of {}",
                account.alias
            ));
        }
    }
    accounts
}

fn load_account(reader: &mut Reader<'_>, alias: &str, shared_seed: Option<&str>) -> Account {
    let upper = alias.to_ascii_uppercase();
    let name = |field: &str| format!("DHAN_{upper}_{field}");
    let own_seed_name = format!("TOKEN_API_{upper}_TOTP_SECRET");

    let client_id = reader.printable(&name("CLIENT_ID"));
    let api_key = reader.printable(&name("API_KEY"));
    let api_secret = reader.printable(&name("API_SECRET"));
    let account_id = reader.printable(&name("ACCOUNT_ID"));
    let pin = reader.printable(&name("ACCOUNT_PIN"));
    let dhan_totp_seed = reader.seed(&name("TOTP_SECRET"));

    let (api_totp_seed, api_seed_shared) = if text(reader.read, &own_seed_name).is_some() {
        (reader.seed(&own_seed_name), false)
    } else if shared_seed.is_some() {
        (reader.seed("TOKEN_API_TOTP_SECRET"), true)
    } else {
        reader.problems.push(format!(
            "{own_seed_name} (or the shared TOKEN_API_TOTP_SECRET) is missing"
        ));
        (Vec::new(), false)
    };

    if !api_totp_seed.is_empty() && api_totp_seed == dhan_totp_seed {
        reader.problems.push(format!(
            "the token API seed for {alias} equals its Dhan seed; a code that fetches a token must not log in to Dhan"
        ));
    }

    Account {
        alias: alias.to_owned(),
        client_id,
        api_key,
        api_secret,
        account_id,
        pin,
        dhan_totp_seed,
        api_totp_seed,
        api_seed_shared,
    }
}

fn load_redirect(reader: &mut Reader<'_>) -> Option<Url> {
    let value = reader.required("DHAN_REDIRECT_URL");
    if value.is_empty() {
        return None;
    }
    match crate::dhan::callback::parse_redirect(&value) {
        Ok(url) => Some(url),
        Err(error) => {
            reader.problems.push(format!("DHAN_REDIRECT_URL: {error}"));
            None
        }
    }
}

fn load_window(reader: &mut Reader<'_>) -> Option<LoginWindow> {
    let window = text(reader.read, "CBL_LOGIN_WINDOW").unwrap_or_else(|| "08:00-08:30".into());
    let days = text(reader.read, "CBL_LOGIN_DAYS").unwrap_or_else(|| "mon-fri".into());
    match LoginWindow::parse(&window, &days) {
        Ok(parsed) => Some(parsed),
        Err(error) => {
            reader
                .problems
                .push(format!("CBL_LOGIN_WINDOW / CBL_LOGIN_DAYS: {error}"));
            None
        }
    }
}

fn load_telegram(reader: &mut Reader<'_>) -> Option<TelegramConfig> {
    let token = text(reader.read, "TELEGRAM_BOT_TOKEN");
    let chat = text(reader.read, "TELEGRAM_CHAT_ID");
    match (token, chat) {
        (Some(bot_token), Some(chat_id)) => Some(TelegramConfig {
            bot_token,
            chat_id,
            api_base: text(reader.read, "CBL_TELEGRAM_API")
                .unwrap_or_else(|| DEFAULT_TELEGRAM_API.into()),
        }),
        (None, None) => None,
        _ => {
            reader
                .problems
                .push("TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID must be set together".into());
            None
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/config.rs"]
mod tests;
