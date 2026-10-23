# Decisions

**What is decided, and what is still open.** Only the operator makes or changes a decision; an agent records it. A changed decision replaces its row, and the old one moves to "Replaced" with the date and the reason. When a decision is built, its status names the commit.

## Decided

| # | Decision | Decided | Status |
| --- | --- | --- | --- |
| A1 | This repository becomes the centralised broker login service: unattended broker logins, an encrypted token store, Telegram notices, a TOTP-gated token API and a login-flow dashboard. Trading engines stop holding broker secrets. | 2026-10-22 | built: `0fd45ee` … `54b3cac` |
| A2 | Backend in Rust; dashboard in React with the reference console's look; the login browser stays a separate Playwright sidecar (copied from `blackbox_trage`). | 2026-10-22 | built |
| A3 | Deployed with Docker on the home VPS `beonedge`, under `/srv/dev_stack/CENTRALISED_BROKER_LOGIN`. Backups and logs under `/srv/backup/CENTRALISED_BROKER_LOGIN`, following the sibling stacks. | 2026-10-22 | built: `523e6e6`, `abc4a0c` |
| A4 | Releases go through `release_manager/`, adapted from `blackbox_trage`. **`status.sh` is the only script an operator runs**: an interactive menu, with `--do <op>` for a phone or agent session. | 2026-10-22 | built: `523e6e6` |
| A5 | The token API takes the account's **Client ID** and a **TOTP** whose seed lives in the `.env`, over HTTPS. | 2026-10-22 | built: `2635bfc` |
| A6 | Public access via a `beonedge.in` subdomain through Cloudflare Tunnel, with Google sign-in via Google Cloud Console and Cloudflare Access. Every login step is automated, as in the reference. | 2026-10-22 | code built; dashboard setup is manual ([deployment.md](manual/deployment.md)) |
| A7 | Hardening against bots and crawlers is done **after** the code base is built (phase 6). | 2026-10-22 | checklist in [security.md](manual/security.md) |
| A8 | The plan ([plan.md](plan.md)) is approved; D1–D9 stay open until the operator addresses them, and everything else is built. | 2026-10-22 | — |
| A9 | Git commit bodies are the development log. | 2026-10-22 | in force |
| A10 | (was D5) The subdomain is a **random 12-character name** under `beonedge.in`, so scanners and wordlists don't find it. Because this repository is public, **the name is never committed**: it lives only in the VPS `.env`, the tunnel config, the nginx site and Cloudflare DNS. Docs and files use `<subdomain>` or `__CBL_HOSTNAME__`. Obscurity only keeps crawlers away; Access and the TOTP gate remain the protection. | 2026-10-23 | `edge/` templates; see [deployment.md](manual/deployment.md) step 1 |

## Open

Each row says what the code does **meanwhile**. Every open decision is a configuration switch, not a code path: deciding one is an edit to the VPS `.env` or a Cloudflare setting, plus a line here.

| # | Question | Recommendation | Meanwhile (what the code does) |
| --- | --- | --- | --- |
| D1 | Which brokers? | Dhan only for now | Dhan only (`src/dhan/`); another broker would be a sibling module behind the login engine |
| D2 | How many accounts, and does each have its own Dhan API app? | — | any number, via `CBL_ACCOUNTS`; each needs its own key and secret with the shared redirect URL |
| D3 | One API TOTP seed per account, or one shared? | per account | **both work**: `TOKEN_API_<ALIAS>_TOTP_SECRET` wins, else `TOKEN_API_TOTP_SECRET`; the dashboard shows which each account uses |
| D4 | Require a Cloudflare Access service token on `/api/v1` as well as the TOTP? | yes | off: `CBL_API_REQUIRE_ACCESS_JWT=false`. Meanwhile it is client ID + TOTP + per-source lockout + per-IP limit. Turned on, the app requires the Access JWT header to be **present**; Cloudflare verifies the token, and the app verifies the signature only from phase 6 |
| D6 | Which Google accounts may open the dashboard? | your own | Access policy decides; `CBL_ADMIN_EMAILS` empty means anyone Access admits |
| D7 | Replace the legacy Next.js env store (Prakash's) on this branch? | yes, after a word with Prakash | **kept untouched** beside the new service; its CI job still runs; its README is in `docs/legacy/` |
| D8 | Telegram: a new bot, or reuse `blackbox_trage`'s? | a new bot, same chat | any bot via `TELEGRAM_BOT_TOKEN` / `TELEGRAM_CHAT_ID`; without them, notices are skipped and shown as such |
| D9 | Postgres: this stack's own container, or a database in the shared `boe-postgres`? | own container | own container in compose; `CBL_DATABASE_URL` points elsewhere if decided the other way |

## Replaced

None yet.
