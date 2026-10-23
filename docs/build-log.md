# Build log

**What is built, what was verified, and what is open.** A new dated section after every change lands, newest first. The commit bodies carry the detail; this is the map.

## 2026-10-23 (17:10 IST): the first real login, and retries

**The first automated login failed** after 6 s at the browser stage: Dhan answered "Invalid Totp entered, try again". The VPS clock is NTP-synchronised and within a second of Cloudflare's, the code is made exactly as in `blackbox_trage` and sent with at least 25 s of life, so the Dhan TOTP secret saved for the account is the likely cause; the operator checks it. The sidecar's browser closes: `login.mjs` (identical to the reference) closes it in `finally`, `server.mjs` reaps any Chromium left, and no browser process was left in `cbl-login` after the failure.

**Built:** automated logins from the dashboard are tried up to 5 times, 60 s apart, stopping at success or a hopeless failure; a second request while one is retrying is ignored; an unknown account is no longer retried. The tries before the last appear only in the activity log; one Telegram notice after the last try (in the morning window too) asks for a manual login, with the try count and the consents left today. 161 Rust tests (5 new), 23 UI tests.

## 2026-10-23 (16:35 IST): first deploy, live

**Deployed** `0.1.1-dev.33.ga6d2234ed` with `status.sh --do deploy --yes`. The first try (`0.1.1-dev.32`) stopped at step 8/13 before changing anything: the dump step misread a missing Postgres container as a broken one (`docker inspect … || echo missing` yields a newline plus "missing"). Fixed in `a6d2234`; the app code is the same.

**Verified on the box:** `cbl-app`, `cbl-postgres` (healthy) and `cbl-login` up; migrations 1 and 2 applied; `/health` reports `ok` with no problems; 0 accounts loaded; the login window is the operator's 07:45-08:30. **Through Cloudflare:** `/` and `/health` go to the Access login; a real Google sign-in reached the dashboard, so the app verified the Access JWT against the live team certs; `/dhan/callback` reaches the app (410, nothing waiting); `/api/v1/token` without a service token gets 403 from Cloudflare; a LAN request faking `CF-Connecting-IP` gets 403 from nginx; `beonedge.in`, `app` and `beus` still answer 200.

**Telegram, fixed the same afternoon:** "chat not found" was a supergroup ID written without its leading minus. With `-100…` the bot (already a member of the alerts group) delivered the start-up notice after a reload and a dashboard test notice; the operator saw both arrive. **Open:** add the Dhan account on the Accounts page, store its token API seed, and run one manual login.

## 2026-10-23 (afternoon): the Accounts page and verified Access sign-ins

Decided with the operator (A13): Dhan accounts live on the dashboard only, the Access signature check ships with them, and both come before the first deploy.

**Built** (`4cfbc91`, `d4d5cba`): accounts sealed in Postgres (migration 0002) and loaded at start with retries; a live registry the engine, scheduler and token API share, so changes need no restart; per-account token API seeds generated and shown once; add, remove and new-seed routes that need the alias typed back; the Accounts tab. Cloudflare Access JWTs are verified (RS256 via `ring`, cached team certs, issuer and AUD), closing the local header-forgery gap. Responses without their own policy get `Cache-Control: no-cache`, so a deploy reaches browsers without a hard reload. The env templates lost the nine account keys and gained `CBL_ACCESS_TEAM_DOMAIN`, `CBL_ACCESS_DASHBOARD_AUD`, `CBL_ACCESS_API_AUD` (20 keys).

**Verified:** 154 Rust tests on a fresh Postgres (from 127), 22 UI tests at 98.3% of lines, clippy `-D warnings` and `cargo audit` clean; the page checked in a browser against the real binary, at desktop and 375 px.

**For the VPS `.env`:** fill the three `CBL_ACCESS_*` values and `CBL_ADMIN_EMAILS`; the old `CBL_ACCOUNTS`, `DHAN_MAIN_*` and `TOKEN_API_*` lines can be deleted (they are ignored, and named in a warning while they still have a value).

## 2026-10-23: the tunnel is locally managed

Opening the Cloudflare dashboard showed the VPS tunnel **beus** is locally managed: its routes are in `/home/beonedge/.cloudflared/config.yml`, and every hostname goes to the host nginx on `:80`. The docs had assumed a dashboard-managed tunnel pointing straight at `127.0.0.1:47440`. Added `release_manager/stacks/broker_login/edge/` (the nginx vhost, which admits only loopback, and the tunnel rule) and rewrote step 1 of [deployment.md](manual/deployment.md) as four commands. Two of them need your sudo, and one restarts `cloudflared`. Nothing on the VPS or in Cloudflare is changed yet.

D5 is decided (A10): a random 12-character subdomain. The repository is public, so the name is never committed; the `edge/` files carry `__CBL_HOSTNAME__`, and step 1 fills it in from `$CBL_HOST` at install time.

**Step 1 done on the VPS (2026-10-23 14:45 IST):** tunnel rule added (backup `config.yml.bak-20261023-143824`) and validated, DNS routed, nginx site installed, cloudflared restarted. The first install answered 403 to everything: the box's `conf.d/boe-shared.conf` rewrites `$remote_addr` from `CF-Connecting-IP`, so the vhost now checks `$realip_remote_addr` (`b2afa07`). **Verified:** through Cloudflare the host answers 502 (nginx reached, no app yet); from the LAN and over Tailscale it answers 403, even with a forged `CF-Connecting-IP`; the five existing sites answer as before. **Still before the first deploy:** the Access applications (step 3).

**Access done:** D6 and D4 decided (A11, A12). Google was already a login method. Created `broker-login` (only me, Google, instant auth), `broker-login-dhan` (dhan-bypass) and `broker-login-api` with a new Service Auth policy `broker-engines`. Verified from outside: `/` redirects to the Access login, `/dhan/callback` reaches nginx, `/api/v1/token` gets 403 without a valid service token. **Left before the first deploy:** the VPS roots (`sudo install -d`), the `.env` (with `CBL_API_REQUIRE_ACCESS_JWT=true` and the real `DHAN_REDIRECT_URL`), and each account's Dhan redirect URL (step 4).

## 2026-10-22 (evening): CI's first push of the branch

The Rust job failed on its empty Postgres: two tests opened the store at once and raced on `CREATE TABLE IF NOT EXISTS`, so one start was refused. Local runs passed only because the database was already migrated. Two instances starting together on the VPS could hit the same race, so the fix is in the store, not the tests: all pending migrations now run in one transaction behind `pg_advisory_xact_lock`. A new test starts six stores together on an empty schema and fails without the lock. 127 Rust tests pass on a fresh database.

## 2026-10-22 (later): security review fixes

A security review of the whole branch found no critical issues. It found one HIGH and eight MEDIUM issues, all addressed in commits `4b003bb`, `35fc176`, `4480944`, `7ef9ba5`, `86fc454` and `022bc72`:

- **HIGH: lockout as denial of service.** The client ID is public, so five wrong codes every ten minutes could keep an engine from its token. Lockouts are now per (client ID, source), with a global budget of 30 failures per client and per shared seed.
- **Enumeration** through the constant decoy seed: the decoy is now random per process and can never pass.
- **Unbounded state:** the guard maps are capped and fail closed; rate-limited requests write nothing; retention prunes rows after 90 or 30 days; lockout alerts are throttled.
- **Postgres held the whole `.env`;** it now gets only its own three variables.
- **Callback spam could abort a manual login:** per-source caps, dedup, and rejected exchanges no longer end the wait.
- **Whole client IDs on the dashboard** (found here first): masked.
- **Hardening:** redacting `Debug`, timestamps bound into the AEAD, a sidecar consent-host allowlist, clipped page text, the identity check in unconfigured mode, charset-checked contract values, a read-only app container with no capabilities, log dirs 750, and `cargo audit` and `npm audit` in CI.

**Left, and said plainly in [security.md](manual/security.md):** Access headers are trusted, not verified, so a local process on the box could forge them (phase 6, JWT verification). The D4 flag checks header presence only. Replay state is in memory. The sidecar installs Playwright at first start.

**Verified:** see the numbers in the next commit; the hardened compose file ran locally (read-only root, `CapDrop=[ALL]`, Postgres environment free of secrets); `cargo audit` and both `npm audit`s report 0 advisories.

## 2026-10-22: the service, end to end

Branch `tamagami-hi/dev`, commits `523e6e6` … this one.

**Built**

- `release_manager/`: the single interactive `status.sh`, plus the VPS-native deploy, rollback and reload; path contract for `/srv/dev_stack/CENTRALISED_BROKER_LOGIN`.
- Rust service `broker_login`: config, TOTP, AES-GCM vault, Dhan consent/exchange/callback, the sidecar client, the per-account login engine and morning scheduler, Postgres and in-memory stores, Telegram, the token API and guard, the dashboard routes and SSE.
- React dashboard with the per-account stage chain.
- The Playwright sidecar, copied from `blackbox_trage`.
- Dockerfile (34 MiB image), local compose, CI.
- Docs: this folder, and the root README.

**Verified (numbers, not percentages remembered)**

- Rust: 115 tests, including the RFC 6238 vectors; line coverage 92.7% with Postgres included; clippy `-D warnings` clean.
- UI: 15 tests, 99.6% of lines; typecheck and build clean. Sidecar: 6 tests.
- `status.sh --status` and `--diagnose` against the real VPS; diagnose names exactly two blockers (below).
- `status.sh --do export`: image built, acceptance passed, bundle staged.
- The deployed compose file run locally with the shipped image and Postgres: healthy, migration applied, Postgres internal-only, port loopback-only. The deploy's `pg_dump` command produced a mode 600 dump.
- `status.sh --do ship` stopped at preflight on the VPS and wrote nothing.
- The dashboard checked in a browser at desktop and 375 px against the real binary.

**Corrected along the way**

- A config error for a bad window didn't name `CBL_LOGIN_WINDOW`; the message was fixed.
- The store contract test assumed an empty database; a second run on a shared one exposed it. The test was scoped to its own rows; the store was right.
- The scheduler would have repeated "everyone logged in" on every restart after the window. It now stays quiet unless an account is missing.

**Open**

- D1–D9 ([decisions.md](decisions.md)).
- **Blocking the first deploy** (needs the operator's sudo; the scripts never run it):

  ```bash
  ssh -t beonedge sudo install -d -o beonedge -g beonedge -m 750 /srv/dev_stack/CENTRALISED_BROKER_LOGIN /srv/backup/CENTRALISED_BROKER_LOGIN
  ```

  Then place `.env`.
- The dashboard-side setup in [deployment.md](manual/deployment.md): tunnel hostname, Google OAuth client, three Access apps, Dhan redirect URLs, Telegram bot.
- Not yet exercised against real Dhan: the consent page as the sidecar sees it today. The sidecar is the reference's, which logs in daily in production, but the first real login on this box is the proof.
- Phase 6 hardening and phase 7 (switching `blackbox_trage` to fetch from here).
