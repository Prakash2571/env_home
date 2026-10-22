# Running and configuration

## Local development

```bash
cp config/env.example .env            # fill it in; .env is gitignored
docker run -d --rm --name cbl-dev-pg -p 127.0.0.1:55432:5432 \
  -e POSTGRES_PASSWORD=dev -e POSTGRES_USER=broker_login -e POSTGRES_DB=broker_login postgres:17-alpine
(cd ui && npm ci && npm run build)    # or run `npm run dev` alongside, on :5180
cargo run                             # serves the API and ui/dist on 127.0.0.1:47440
```

For a quick look without Postgres, set `CBL_STORE=memory` (nothing survives a restart). For the browser sidecar locally, run it on `:8090`:

```bash
cd sidecar
npm ci
npx playwright install chromium
node server.mjs
```

Set `DHAN_BROWSER_HEADED=true` to watch it work.

Without a complete configuration, the binary still starts. It logs `configuration incomplete: …` with every missing variable, serves `/health` as `unconfigured`, and the dashboard lists the problems. That is deliberate: a missing secret is not fixed by a rollback, so it must not look like a crash to the deploy gate.

## Environment variables

Every key is in both [config/env.example](../../config/env.example) and the deployed [.env.example](../../release_manager/stacks/broker_login/.env.example). The export refuses to ship a bundle if the two differ.

| Variable | Required | Default | Meaning |
| --- | --- | --- | --- |
| `CBL_ENCRYPTION_KEY` | yes | — | 64 hex characters; encrypts tokens at rest |
| `POSTGRES_PASSWORD` | yes (Postgres) | — | also read by the Postgres container |
| `POSTGRES_DB`, `POSTGRES_USER` | | `broker_login` | |
| `CBL_DATABASE_HOST`, `CBL_DATABASE_PORT` | | `127.0.0.1`, `5432` | compose sets the host to `postgres` |
| `CBL_DATABASE_URL` | | — | overrides the above; any libpq string (D9) |
| `CBL_STORE` | | `postgres` | `memory` for throwaway local runs |
| `CBL_ACCOUNTS` | yes | — | comma-separated aliases (letters, digits, `_`), logged in in this order |
| `DHAN_<ALIAS>_CLIENT_ID` | yes | — | the Dhan client ID; also what an engine sends to the token API |
| `DHAN_<ALIAS>_API_KEY`, `_API_SECRET` | yes | — | from that account's Dhan API app |
| `DHAN_<ALIAS>_ACCOUNT_ID` | yes | — | what the login form's first box takes (mobile number or client ID) |
| `DHAN_<ALIAS>_ACCOUNT_PIN` | yes | — | the account PIN |
| `DHAN_<ALIAS>_TOTP_SECRET` | yes | — | base32 Dhan authenticator seed |
| `TOKEN_API_<ALIAS>_TOTP_SECRET` | one of these two | — | the seed engines use for this account (D3) |
| `TOKEN_API_TOTP_SECRET` | one of these two | — | one shared seed for every account without its own (D3) |
| `DHAN_REDIRECT_URL` | yes | — | the public callback registered with every Dhan API app; its path becomes the route |
| `CBL_SIDECAR_URL` | | — | e.g. `http://login:8090/login`; without it only manual logins work |
| `CBL_LOGIN_WINDOW` | | `08:00-08:30` | IST |
| `CBL_LOGIN_DAYS` | | `mon-fri` | lists and ranges: `mon,wed-fri` |
| `CBL_HTTP_ADDR` | | `127.0.0.1:47440` | the image sets `0.0.0.0:47440` |
| `CBL_HOME` | | `.` | the UI is served from `$CBL_HOME/ui/dist` |
| `CBL_ADMIN_REQUIRE_ACCESS` | | `true` | the dashboard API needs the Access identity |
| `CBL_ADMIN_EMAILS` | | — | allowlist (D6) |
| `CBL_API_REQUIRE_ACCESS_JWT` | | `false` | token API demands Access's header too (D4) |
| `CBL_CLIENT_IP_HEADER` | | `cf-connecting-ip` | `none` to use the socket peer |
| `TELEGRAM_BOT_TOKEN`, `TELEGRAM_CHAT_ID` | together or neither | — | D8 |
| `CBL_DHAN_AUTH_BASE`, `CBL_TELEGRAM_API` | | the real ones | test-only overrides |
| `RUST_LOG` | | `info` | |

## Tests

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
CBL_TEST_DATABASE_URL="host=127.0.0.1 port=55432 user=broker_login password=dev dbname=broker_login" cargo test store
cargo llvm-cov --lib --summary-only
node --test tests/sidecar/*.test.mjs
(cd ui && npm run typecheck && npm test && npm run build)
```

The Postgres contract test is skipped unless `CBL_TEST_DATABASE_URL` is set. CI sets it.
