use std::net::SocketAddr;
use std::path::PathBuf;

use reqwest::Url;

use crate::access::AccessVerifier;
use crate::clock::LoginWindow;

pub const DEFAULT_HTTP_ADDR: &str = "127.0.0.1:47440";
pub const DEFAULT_DHAN_AUTH_BASE: &str = "https://auth.dhan.co";
pub const DEFAULT_TELEGRAM_API: &str = "https://api.telegram.org";

#[derive(Clone, Debug)]
pub enum StoreConfig {
    Memory,
    Postgres(String),
}

#[derive(Clone)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub chat_id: String,
    pub api_base: String,
}

/// A Cloudflare Access check: the team's verifier and one application's AUD tag.
#[derive(Clone, Debug)]
pub struct AccessCheck {
    pub verifier: AccessVerifier,
    pub audience: String,
}

#[derive(Clone, Debug)]
pub struct ApiPolicy {
    pub require_access_jwt: bool,
    pub client_ip_header: Option<String>,
    pub access: Option<AccessCheck>,
}

#[derive(Clone, Debug)]
pub struct AdminPolicy {
    pub require_access_identity: bool,
    pub allowed_emails: Vec<String>,
    pub access: Option<AccessCheck>,
}

#[derive(Clone)]
pub struct Config {
    pub http_addr: SocketAddr,
    pub home: PathBuf,
    pub store: StoreConfig,
    pub encryption_key: String,
    pub redirect_url: Url,
    pub sidecar_url: Option<String>,
    pub dhan_auth_base: String,
    pub telegram: Option<TelegramConfig>,
    pub window: LoginWindow,
    pub api: ApiPolicy,
    pub admin: AdminPolicy,
}

impl std::fmt::Debug for TelegramConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TelegramConfig")
            .field("chat_id", &self.chat_id)
            .field("api_base", &self.api_base)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for Config {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Config")
            .field("http_addr", &self.http_addr)
            .field("redirect_url", &self.redirect_url.as_str())
            .field("sidecar_url", &self.sidecar_url)
            .field("telegram", &self.telegram)
            .field("window", &self.window.label())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConfigGap {
    pub problems: Vec<String>,
}

pub struct Basics {
    pub http_addr: SocketAddr,
    pub home: PathBuf,
    pub admin: AdminPolicy,
    pub access: Option<AccessVerifier>,
}

const ACCESS_TEAM_SUFFIX: &str = ".cloudflareaccess.com";

/// The team's Access origin and nothing else: it is the trust anchor for
/// every sign-in, so a path, a query or another host is a configuration error.
fn team_domain(read: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    text(read, "CBL_ACCESS_TEAM_DOMAIN").filter(|value| {
        Url::parse(value).is_ok_and(|url| {
            url.scheme() == "https"
                && url.host_str().is_some_and(|host| {
                    host.ends_with(ACCESS_TEAM_SUFFIX) && host.len() > ACCESS_TEAM_SUFFIX.len()
                })
                && url.port().is_none()
                && matches!(url.path(), "" | "/")
                && url.query().is_none()
                && url.fragment().is_none()
                && url.username().is_empty()
        })
    })
}

pub fn access_verifier(read: &dyn Fn(&str) -> Option<String>) -> Option<AccessVerifier> {
    team_domain(read).and_then(|domain| AccessVerifier::new(&domain).ok())
}

fn access_check(
    read: &dyn Fn(&str) -> Option<String>,
    verifier: Option<&AccessVerifier>,
    audience_variable: &str,
) -> Option<AccessCheck> {
    Some(AccessCheck {
        verifier: verifier?.clone(),
        audience: text(read, audience_variable)?,
    })
}

pub fn admin_policy(
    read: &dyn Fn(&str) -> Option<String>,
    verifier: Option<&AccessVerifier>,
) -> AdminPolicy {
    AdminPolicy {
        access: access_check(read, verifier, "CBL_ACCESS_DASHBOARD_AUD"),
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
    }
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
    let access = access_verifier(read);
    Basics {
        http_addr,
        home,
        admin: admin_policy(read, access.as_ref()),
        access,
    }
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
}

/// Account variables from before accounts moved to the dashboard. They are
/// ignored; the service names any that still carry a value so they get removed.
const RETIRED_VARIABLES: [&str; 2] = ["CBL_ACCOUNTS", "TOKEN_API_TOTP_SECRET"];

pub fn retired_variables(read: &dyn Fn(&str) -> Option<String>) -> Vec<&'static str> {
    RETIRED_VARIABLES
        .into_iter()
        .filter(|name| text(read, name).is_some())
        .collect()
}

pub fn load(read: &dyn Fn(&str) -> Option<String>) -> Result<Config, ConfigGap> {
    let mut reader = Reader {
        read,
        problems: Vec::new(),
    };
    let Basics {
        http_addr,
        home,
        admin,
        access,
    } = basics(read);
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

    let redirect_url = load_redirect(&mut reader);
    let window = load_window(&mut reader);
    let telegram = load_telegram(&mut reader);

    let api = ApiPolicy {
        access: access_check(read, access.as_ref(), "CBL_ACCESS_API_AUD"),
        require_access_jwt: flag(read, "CBL_API_REQUIRE_ACCESS_JWT", false),
        client_ip_header: match text(read, "CBL_CLIENT_IP_HEADER") {
            Some(value) if value.eq_ignore_ascii_case("none") => None,
            Some(value) => Some(value.to_ascii_lowercase()),
            None => Some("cf-connecting-ip".into()),
        },
    };

    check_access(&mut reader, &admin, &api);

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

/// A check that is on but cannot verify anything would refuse every request,
/// so it is a configuration problem rather than a silent lock-out.
fn check_access(reader: &mut Reader<'_>, admin: &AdminPolicy, api: &ApiPolicy) {
    if text(reader.read, "CBL_ACCESS_TEAM_DOMAIN").is_some() && team_domain(reader.read).is_none() {
        reader.problems.push(
            "CBL_ACCESS_TEAM_DOMAIN must be https://<team>.cloudflareaccess.com, with no path or query"
                .into(),
        );
    }
    if admin.require_access_identity && admin.access.is_none() {
        reader.problems.push(
            "CBL_ACCESS_TEAM_DOMAIN and CBL_ACCESS_DASHBOARD_AUD are required while CBL_ADMIN_REQUIRE_ACCESS is on"
                .into(),
        );
    }
    if api.require_access_jwt && api.access.is_none() {
        reader.problems.push(
            "CBL_ACCESS_TEAM_DOMAIN and CBL_ACCESS_API_AUD are required while CBL_API_REQUIRE_ACCESS_JWT is on"
                .into(),
        );
    }
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
