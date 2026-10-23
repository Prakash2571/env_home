# HTTP routes and the token API

Every route the service answers, and the contract a trading engine codes against. **A route added or changed is recorded here in the same change.**

The service listens on `CBL_HTTP_ADDR`, which is `0.0.0.0:47440` inside the container and published only as `127.0.0.1:47440` on the VPS. The public subdomain reaches it through the host's Cloudflare Tunnel and the host nginx vhost (`release_manager/stacks/broker_login/edge/`), which admits only loopback.

## Routes

| Method | Path | Who calls it | Guard |
| --- | --- | --- | --- |
| GET | `/health` | the deploy gate, monitors | none; it reveals only `ok` or `unconfigured` and a count |
| GET | `/robots.txt` | crawlers | none; `Disallow: /` |
| GET | `/dhan/callback` (the path of `DHAN_REDIRECT_URL`) | Dhan's consent page | validated before use; only while a login waits; 5 attempts per source, 200 per login |
| POST | `/api/v1/token` | trading engines | TOTP gate (below) |
| GET | `/api/state` | the dashboard | Access identity |
| GET | `/api/stream` | the dashboard (SSE) | Access identity |
| POST | `/api/accounts` | the Accounts page: add an account | Access identity + JSON body |
| DELETE | `/api/accounts/{alias}` | the Accounts page: remove one | Access identity + JSON body with the alias typed |
| POST | `/api/accounts/{alias}/token-seed` | the Accounts page: new token API seed | Access identity + JSON body with the alias typed |
| POST | `/api/accounts/{alias}/login` | the dashboard | Access identity + JSON body |
| POST | `/api/accounts/login` | the dashboard (every account) | Access identity + JSON body |
| POST | `/api/telegram/test` | the dashboard | Access identity + JSON body |
| GET | anything else | browsers | serves `ui/dist`, with `index.html` as the fallback |

Every response carries `X-Robots-Tag: noindex, nofollow, noarchive, nosnippet`, `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer` and a strict `Content-Security-Policy`. The request body limit is 16 KiB.

**Access identity** means the JWT Cloudflare Access signs into `Cf-Access-Jwt-Assertion`. The app verifies it (RS256 against the team's published certs, issuer `CBL_ACCESS_TEAM_DOMAIN`, audience `CBL_ACCESS_DASHBOARD_AUD`, within its lifetime) and takes the email from it. The plain `Cf-Access-Authenticated-User-Email` header is never read: any process on the box could set it. Without a valid sign-in the dashboard routes answer 401; a service token (no email) gets 403. If `CBL_ADMIN_EMAILS` is set (A11), any other email gets 403. `CBL_ADMIN_REQUIRE_ACCESS=false` turns the check off for local development only.

## The token API

```http
POST /api/v1/token
Content-Type: application/json
CF-Access-Client-Id: <service token id>          (only if D4 is decided that way;
CF-Access-Client-Secret: <service token secret>   Cloudflare checks these and signs a JWT the app verifies)

{"client_id": "1100000001", "totp": "123456"}
```

`totp` is the current six-digit code from the account's **token API seed**. The service generates it when the account is added on the Accounts page and shows it once; **New token seed** replaces it, and the old one stops working at once. It is never the account's Dhan seed.

| Status | Body `error` | Meaning | What the engine does |
| --- | --- | --- | --- |
| 200 | — | `{client_id, access_token, obtained_at, expires_at, expires_in}`; times are IST RFC 3339 | cache the token until `expires_at`, then fetch again |
| 400 | `bad_request` | not JSON, or `client_id`/`totp` malformed | fix the request |
| 401 | `unauthorized` | wrong code, unknown or removed client ID, or (D4 on) no valid Access service token | check the seed and the clock; do not retry in a loop |
| 409 | `code_already_used` | that code was already accepted once | wait for `Retry-After` (the next 30 s step) and send the new code |
| 429 | `locked` | 5 wrong codes in 10 minutes from that source, or 30 from all sources; locked for 10 minutes | wait for `Retry-After`; Telegram was alerted |
| 429 | `rate_limited` | more than 30 requests a minute from one IP | wait 60 s |
| 503 | `no_token_yet` | no valid token for that account yet today | wait for `Retry-After`, at most 5 minutes |
| 503 | `not_configured` | the service is up but not configured | page the operator |

Why it is shaped like this:

- **POST, not GET,** so the code never lands in a URL, a proxy log or a history. Every reply is `Cache-Control: no-store`.
- **An unknown client ID and a wrong code get the same reply and the same lockout.** The unknown one is checked against a random per-process decoy seed that can never pass, so the API can't be used to discover which client IDs exist.
- **A lockout belongs to the source that earned it.** A guesser elsewhere cannot lock the engine out by sending wrong codes for its client ID. Only 30 failures from all sources together lock the client everywhere.
- **One step of clock drift** each way is accepted, so a code from the previous or next 30 s step still passes.
- **A code is accepted once.** Two engines fetching the same account in the same 30 s step: the second gets 409 and retries on the next step. Tokens last about 24 hours, so an engine fetches once a morning and caches.
- **Every request is recorded** (client ID, account, IP, outcome, reason) and shown on the dashboard.

### Client example (Rust, for `blackbox_trage`)

```rust
async fn fetch_dhan_token(base: &str, client_id: &str, seed: &[u8]) -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    for _ in 0..3 {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
        let totp = code_at(seed, now);
        let response = client
            .post(format!("{base}/api/v1/token"))
            .json(&serde_json::json!({ "client_id": client_id, "totp": totp }))
            .send()
            .await?;
        match response.status().as_u16() {
            200 => return Ok(response.json::<serde_json::Value>().await?["access_token"]
                .as_str().unwrap_or_default().to_owned()),
            409 | 503 => {
                let wait = response.headers().get("retry-after")
                    .and_then(|value| value.to_str().ok()?.parse().ok()).unwrap_or(30);
                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
            }
            status => anyhow::bail!("the token API refused with HTTP {status}"),
        }
    }
    anyhow::bail!("no token after three tries")
}
```

`code_at` is the same RFC 6238 function as in `src/otp.rs`. Copy it, or reuse `blackbox_trage`'s `src/otp.rs`, which is where it came from.

### From a shell

```bash
CODE=$(oathtool --totp -b "$TOKEN_API_SEED")   # the seed the Accounts page showed
curl --fail-with-body -sS -X POST https://<subdomain>.beonedge.in/api/v1/token \
  -H 'content-type: application/json' \
  -d "{\"client_id\":\"1100000001\",\"totp\":\"$CODE\"}"
```

Treat the reply as a secret: never print it to a shared log.

## The dashboard routes

- `GET /api/state` returns the snapshot: configured or not (with the problems), the store, the sidecar, the login window, each account's stage chain and token status (never the token, never the whole client ID), Telegram's delivery counts, the activity log, and the last 25 token requests and login attempts.
- `GET /api/stream` is server-sent events. Each `snapshot` event carries the same body as `/api/state` and is sent on every change. Keep-alives come every 15 s.
- `POST /api/accounts/{alias}/login` with `{"drive": "automated"}` runs the sidecar now, at any hour. `{"drive": "manual"}` publishes the consent link on the card for a person to open, and waits 5 minutes for the callback. It returns 202 when queued, 404 for an unknown account, 409 for automated without a sidecar, and 415 without `application/json`.
- `POST /api/telegram/test` queues a test notice; 409 if Telegram is not configured.

## The account routes

All three need a verified Access sign-in and `application/json`. Bodies are parsed by the service itself, so an error never quotes a value back.

- `POST /api/accounts` with `{alias, client_id, api_key, api_secret, account_id, pin, dhan_totp_secret}`. `alias` is up to 32 of `a-z 0-9 _` (not `login`), `client_id` up to 20 letters or digits, `pin` 4 to 8 digits, `dhan_totp_secret` a base32 seed of at least 80 bits, the rest printable without spaces. **201** `{alias, token_api_seed, otpauth_uri}`, `no-store`: the only time the token API seed is shown. **400** `{error, problems: [{field, message}]}`; **409** if the alias or client ID is taken; **503** while the stored accounts are still loading.
- `DELETE /api/accounts/{alias}` with `{"confirm": "<alias>"}`. Deletes the account's sealed secrets and every stored token; its login history stays. **200** `{removed}`; **400** if the alias is not typed exactly; **404**; **409** while a login for it is running.
- `POST /api/accounts/{alias}/token-seed` with `{"confirm": "<alias>"}`. A fresh seed replaces the old one at once. **200** `{alias, token_api_seed, otpauth_uri}`, shown once.

Every change goes to the activity log and, if configured, Telegram. The snapshot's `accounts_loaded` says whether the stored accounts are in yet.
