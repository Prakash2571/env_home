# Build plan: broker login and token service

Status: **approved 2026-10-22; built on `tamagami-hi/dev` (see [build-log.md](build-log.md)).** Decisions are in [decisions.md](decisions.md); several were decided after approval (A10–A13). Section 8 below describes the state when the plan was written.

## 1. What this service is

A small service on the home VPS (`beonedge`) that:

1. **Logs in to broker accounts every morning, unattended.** It drives the broker's OAuth consent page in a headless browser, enters the account ID, PIN and authenticator code, and exchanges the result for an access token.
2. **Stores each account's access token**, encrypted at rest.
3. **Sends Telegram notices** for every login, failure and security event.
4. **Gives tokens to other programs** (trading engines such as `blackbox_trage`) over HTTPS. A program must present the account's **Client ID** and a **current TOTP code**.
5. **Shows the login flow on a web page** at a `beonedge.in` subdomain, with each account's progress drawn stage by stage, like the reference console.

The goal is to take the login system out of the trading engines. An engine stops holding a broker PIN or a TOTP seed. It asks this service for the day's token.

## 2. What is reused from `blackbox_trage`

| Reference code | What it does | How it is used here |
| --- | --- | --- |
| `src/dhan/dhan_oauth/server_callbacks.rs` | Generates the consent, exchanges `tokenId` for the token, checks the expiry | Ported almost unchanged, with one config per account |
| `src/dhan/dhan_oauth/browser.rs` | Calls the sidecar, waits for a fresh TOTP step, backs off after failures, detects "hopeless" (lockout or SMS OTP) | Ported; the state moves from statics into a per-account struct |
| `src/dhan/dhan_oauth/callback.rs` | Validates the public `/dhan/callback` before using it; caps attempts | Ported unchanged |
| `src/dhan/dhan_oauth/mod.rs` | 08:00–08:30 IST window, 5 attempts, daily consent cap, retry gap | Ported and generalised to N accounts, run one after another |
| `src/otp.rs`, `totp.rs` | RFC 6238 TOTP (base32, HMAC-SHA1, 30 s step) | Reused for the broker logins **and** to verify the API's codes |
| `src/telegram/inbound/code.rs` | Checks a code with ±1 step drift, rejects a reused code, locks out after 5 wrong codes in 10 min | Reused as the gate on the token API |
| `src/telegram/{channel,notify,compose,pacing,message}.rs` | Telegram sender: HTML, splitting, 20 msg/min pacing, queue, never leaks the bot token | Ported for outbound notices |
| `sidecar/*.mjs` | Playwright login: finds fields by their labels, fills account / PIN / TOTP, catches `tokenId` at the redirect | Copied as is, still a separate container |
| `ui/src/dhan/LoginPanel.tsx`, `flowchart/`, `LivePipeline.tsx` | Login panel, stage flowchart, live diagnosis | The visual style is reused; the stages become per-account |
| `release_manager/` | export → deploy → VPS-native script, checksums, lock, health gate, auto-rollback | Adapted for this VPS; already partly written (section 8) |

One difference matters. The reference stores the session in a JSON file and has no database on the deploy path. This service keeps tokens for many accounts and serves them to other programs, so its state lives in Postgres. Deploys take a database dump before every change.

## 3. Architecture

```
                    Cloudflare edge (Access + WAF)
                               │  tunnel (already running on the VPS)
                               ▼
beonedge VPS ── 127.0.0.1:47440 ──► app  (Rust: axum API + the built React UI)
                                     │  compose network only
                          ┌──────────┴──────────┐
                          ▼                     ▼
                login (Playwright)        postgres:17 (internal network,
                 no published port         no route off the host)
```

- **app**: one Rust binary (tokio, axum, reqwest with rustls, tokio-postgres). It runs the login scheduler, the token API, Telegram, and the server-sent event (SSE) stream for the UI, and it serves `ui/dist`. Port 47440 is published on loopback only.
- **login**: the pinned Playwright image plus the copied sidecar scripts. It takes a PIN and a live TOTP, so it has no published port.
- **postgres**: its own container, on an internal Docker network that only `app` can reach. It does **not** use the `boe-postgres` that serves the other production apps.
- **Edge**: the `cloudflared` service already running on the VPS is remote-managed (it runs from a token). A new public hostname is added in the Zero Trust dashboard and pointed at `http://127.0.0.1:47440`. There is no Caddy and no nginx in this stack, and no ports 80/443 of its own. *(Corrected 2026-10-23: the tunnel is locally managed and routes through the host nginx; see [deployment.md](manual/deployment.md), step 1.)*

All three containers run as Docker Compose project `env_home`, under `/home/beonedge/env_home`. The VPS has room: 8 cores, 14 GiB RAM, 400 GB free.

## 4. The token API

```
POST /api/v1/token
CF-Access-Client-Id: …          ← Cloudflare service token (recommended, see D4)
CF-Access-Client-Secret: …
Content-Type: application/json

{ "client_id": "1100xxxxxx", "totp": "123456" }

200 { "client_id": "…", "access_token": "…", "expires_at": "…", "obtained_at": "…" }
```

- It is a **POST**, so the code never lands in a URL, a proxy log or a browser history. Every response carries `Cache-Control: no-store`.
- **The Client ID is not a secret.** It is printed on contract notes and statements, so the TOTP is the real lock. Six digits with ±1 step drift means a guess wins 3 times in 1,000,000. That is safe only with all of the following:
  - **Lockout:** 5 wrong codes for a client ID within 10 minutes locks that client ID for 10 minutes. This is the reference `CodeGuard`. Each lockout sends a Telegram alert.
  - **Replay refusal:** a code is accepted once per client ID per 30 s step.
  - **Rate limit:** a per-IP limit in the app, plus a Cloudflare rate-limit rule.
  - **Uniform 401:** an unknown client ID and a wrong code get the same reply, so client IDs can't be enumerated.
  - **API seed kept separate:** the API's TOTP seed is never a broker's TOTP seed. A code that can fetch a token must not be able to log in to Dhan. The reference made the same rule for its Telegram commands.
- **Two engines asking in the same 30 s:** the second one gets `409 code already used` and retries on the next step, at most 30 s later. Tokens last about 24 h, so an engine fetches once a morning and caches. A client snippet for `blackbox_trage` handles this.
- **No token yet today:** `503` with `Retry-After` pointing at the end of the login window.
- **Audit:** every request is recorded (client ID, IP, outcome, reason). The UI shows the trail, and Telegram sends a daily summary.

## 5. The login flow, per account

1. Inside the 08:00–08:30 IST window on trading days, the accounts log in **one after another**. The sidecar holds one browser at a time.
2. For each account: generate a consent with that account's Dhan API key and secret. Then the sidecar fills the account ID, PIN and a fresh TOTP code. It catches `tokenId` at `https://<subdomain>/dhan/callback`, the code exchanges it, the token is encrypted and stored, and Telegram is notified.
3. On failure: retry with backoff, at most 5 tries in the window, and stop early if the failure is hopeless (an SMS OTP or a lockout). Telegram sends the error and the next step.
4. Manual paths from the UI: **Run automated login now** for one account or all, and **Open consent link** to log in by hand.
5. Each account needs **its own Dhan API app**, registered with the same redirect URL. The registered URL is the public `/dhan/callback`, which must bypass Cloudflare Access because Dhan's servers can't sign in.

The broker credentials live in the VPS `.env`, one prefix per account, the same pattern as the reference: `ACCOUNTS=main,helper2,…`, then `DHAN_MAIN_CLIENT_ID`, `_API_KEY`, `_API_SECRET`, `_ACCOUNT_PIN`, `_TOTP_SECRET`, and the API seed `TOKEN_API_MAIN_TOTP_SECRET`. The UI never accepts or shows these values. *(Replaced 2026-10-23 by A13: accounts are added on the dashboard's Accounts page, sealed in Postgres; the UI accepts these values once and never shows them. See [decisions.md](decisions.md).)*

## 6. The web page

React, Vite and TypeScript, built into the app image and styled after the reference console. It updates live over SSE.

- **Accounts board:** one card per account with its stage chain: consent → browser → account ID → PIN → TOTP → callback → token exchange → stored → notified. Each stage shows its state, duration, attempts and last error, the same idea as the reference flowchart.
- **Token status per account:** minted at, valid for, and which consumers fetched it. The token itself is never shown.
- **Actions:** run a login, open the consent link, and a "test Telegram" button.
- **Access log:** recent token API requests and lockouts.

Only Google accounts you list can open the page, enforced by Cloudflare Access (section 7).

## 7. Deployment and the public edge

Done once, mostly by hand in dashboards. `docs/deployment.md` will give the exact clicks.

1. **Subdomain:** pick one (D5), e.g. `login.beonedge.in`.
2. **Tunnel:** in Zero Trust → Networks → Tunnels, add a public hostname to the existing tunnel and point it at `http://127.0.0.1:47440`. DNS is created automatically.
3. **Google Cloud Console:** create an OAuth client (Web application). Its redirect URI is `https://<team>.cloudflareaccess.com/cdn-cgi/access/callback`. Add it to Zero Trust as the Google login method.
4. **Cloudflare Access applications:**
   - `<subdomain>/*`: Google login, Allow only your listed emails.
   - `<subdomain>/dhan/*`: **Bypass**, so Dhan's callback reaches the app.
   - `<subdomain>/api/v1/*`: **Service Auth**, where each trading engine gets its own service token (D4).
5. **Dhan dashboard:** set each account's API app redirect URL to `https://<subdomain>/dhan/callback`.
6. **Telegram:** create a bot with BotFather and add it to your chat.
7. **VPS `.env`:** you place it at `/home/beonedge/env_home/app/.env`. No script reads, writes or deletes it.

**Releases** use `release_manager/`, adapted from the reference:

```bash
./release_manager/provision.sh --app --beonedge   # once: create the directory tree
./release_manager/export.sh    --app --beonedge   # build the image locally, stage a bundle
./release_manager/deploy.sh    --app --beonedge   # upload over Tailscale, deploy on the VPS
./release_manager/status.sh    --app --beonedge   # compare local and live, read-only
./release_manager/rollback.sh  --app --beonedge --list
```

What changed from the reference, and why:

- **One host, still with no default.** The VPS also runs the boe-*, market-data-* and aie-* production services. Every Docker command is scoped to this project, its container names or its own image tags, and nothing prunes globally.
- **No Caddy, no origin certificates, no static site.** Cloudflare Tunnel is the edge. The deploy only *reports* whether `cloudflared` is active and never restarts it, because restarting it would drop every other site on the box.
- **A database dump before every deploy.** It is written to `backup/db/` (mode 700, the last 14 kept). Auto-rollback stays image-only. Migrations must be backward compatible with the previous release (add first, remove one release later), so the old image still runs against the new schema. Restoring a dump is a manual step, because it would lose tokens minted since.
- **Health is an HTTP probe** of `http://127.0.0.1:47440/health`.
- **Deploys go over Tailscale** (`ssh beonedge`), never the public SSH alias.

## 8. Work already done on this branch (uncommitted)

- `release_manager/lib/`: `ui.sh` and `version.sh` (copied), and `stacks.sh`, `hosts.sh` and `paths.sh` (adapted).
- `release_manager/stacks/env_home/paths.json`: the path contract for `/home/beonedge/env_home`.
- `release_manager/stacks/_shared/_eh_lib.sh`: the VPS-side runtime (lock, checksums, database dump, image load and archive, health gate, edge report, scoped pruning).
- Still to write: `export.sh`, `deploy.sh`, `rollback.sh`, `status.sh`, `provision.sh`, `resources.sh`, the shared deploy and rollback flows, the compose file, `Dockerfile`, the README and guide.

## 9. Phases

Each phase ends with tests passing and a commit on `tamagami-hi/dev`. The phases follow your rules: tests written first, at least 80% coverage, and a review pass.

| # | Phase | Done when |
| --- | --- | --- |
| 0 | **Foundation**: replace the Next.js env store with the Rust + `ui/` + `sidecar/` layout. Finish `release_manager/`, add `Dockerfile` and compose, and set up CI (fmt, clippy, tests, UI tests, Docker build). The app serves `/health` and an empty UI. | A hello-world release is deployed to the VPS by the pipeline and reachable through the tunnel behind Google login |
| 1 | **Login engine**: accounts from `.env`, the Dhan OAuth port, the sidecar, encrypted token storage in Postgres, the scheduler window, retries and the hopeless stop | Mocked Dhan and sidecar tests pass, and one real account logs in on the VPS |
| 2 | **Telegram**: the notice builder, pacing and queue; login success and failure, a morning summary, lockout alerts | Real messages arrive in the chat |
| 3 | **Token API**: the TOTP gate, lockout, replay refusal, rate limit, audit table; a Rust client snippet for `blackbox_trage` | A consumer fetches a token with the service token, client ID and TOTP; wrong codes lock out and alert |
| 4 | **UI**: the accounts board, stage chains, actions, token status, access log over SSE | The flow is visible live during a login |
| 5 | **Go live**: subdomain, tunnel hostname, the three Access apps, the Google OAuth client, Dhan redirect URLs, the VPS `.env` | The morning window runs unattended for all accounts |
| 6 | **Hardening (later, as you said)**: Cloudflare bot-fight and WAF rules, edge rate limits, verifying the `Cf-Access-Jwt-Assertion` header in the app, `robots.txt`/`X-Robots-Tag`, alerting if the service is down | Agreed checklist done |
| 7 | **Switch `blackbox_trage`** to fetch from this service and remove its own login (a separate PR in that repo) | The engine trades on a token from this service |

## 10. Decisions needed from you

My recommendation comes first in each.

- **D1: brokers.** Dhan only for now, behind a `Broker` trait so another broker can be added later. Or name the others now.
- **D2: accounts.** How many Dhan accounts, and does each already have its own API app (key and secret)?
- **D3: API TOTP seed.** **One seed per account** (recommended: a leaked seed exposes one account and is rotated alone). Or one shared seed for all accounts, as you first described.
- **D4: extra edge factor for the API.** **Require a Cloudflare Access service token per consumer** (recommended: it blocks crawlers and scanners before they reach the app, and each engine can be revoked on its own). Or Client ID + TOTP only.
- **D5: subdomain name**, e.g. `login.beonedge.in` or `tokens.beonedge.in`.
- **D6: who can open the UI.** The list of Google emails.
- **D7: the existing Next.js code** on `main` (Prakash's env store). Replace it on this branch and open a PR after phase 0 (recommended), or keep it in a subfolder. Worth a word with Prakash first.
- **D8: Telegram.** A **new bot**, posting to your existing chat (recommended), or reuse the `blackbox_trage` bot.
- **D9: Postgres.** **Its own container** in this stack (recommended), or a database inside the shared `boe-postgres`.
