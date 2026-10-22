# Centralised broker login

One service on the home VPS (`beonedge`) logs every Dhan account in each morning, unattended. It keeps each access token encrypted, reports to Telegram, and hands a token to a trading engine that proves itself with the account's **Client ID** and a **current TOTP code**. Trading engines such as `blackbox_trage` stop holding broker PINs and seeds; they ask this service. A dashboard at a `beonedge.in` subdomain draws each account's login flow live.

Private project, one operator. **This file governs how work is done here:** the rules, how to prepare context, and how the docs are kept. What the service is and does lives in the documents it points to.

```
Rust service (src/)  ─┬─ the login engine + morning scheduler ── Playwright sidecar (sidecar/)
                      ├─ the token API  (POST /api/v1/token, TOTP-gated)
                      ├─ the dashboard API + SSE ── React dashboard (ui/)
                      ├─ Postgres (encrypted tokens, attempts, audit)
                      └─ Telegram notices
deployed by release_manager/status.sh to /srv/dev_stack/CENTRALISED_BROKER_LOGIN, behind Cloudflare Tunnel + Access
```

## Authority

Highest first:

1. [docs/decisions.md](docs/decisions.md): **what is decided** (A-rows) and **what is open** (D1–D9), with what the code does meanwhile. Read it whole.
2. [docs/build-log.md](docs/build-log.md): **what is built**, what was verified, and what is open.
3. The `git log` bodies: the design record. Each commit body explains why, what was verified and what was corrected.
4. [docs/plan.md](docs/plan.md): the approved plan and its phases.
5. `docs/manual/`: the operating reference.

## Rules

**Safety. Breaking any of these fails the work, whatever the tests say.**

1. **Never deploy unasked.** Images are built locally; the operator runs `release_manager/status.sh`. An agent exports or ships only when asked, and never runs `sudo` on the VPS.
2. **`.env` belongs to the operator.** Never print its values, never commit one, never change a credential. No script reads, writes or removes the VPS `.env`.
3. **A secret never leaves its place.** Tokens, PINs, seeds, API secrets and the bot token never go in a log, an error message, a Telegram notice, the dashboard, a URL or a commit. The dashboard shows a client ID only as `••••1234`.
4. **A token API seed is never a broker seed.** The service refuses to start if they match; never weaken that check.
5. **The token API refuses alike.** An unknown client ID and a wrong code get the same reply and the same lockout. Never add a reply that tells them apart.
6. **This VPS is shared.** Every docker command is scoped to project `centralised_broker_login`, its `cbl-*` containers and its `cbl-*` images. Never prune globally, and never restart `cloudflared`.
7. **Migrations stay compatible with the previous release** (add a column in one release, drop it a later one), because rollback swaps images and keeps the database.
8. **Open decisions stay open.** D1–D9 are the operator's. Build behind configuration, take the conservative default, and say which decision a switch belongs to. Never decide one in code.
9. **Never weaken a failing test.** Decide which side is wrong and say so in the commit.

**Working.**

10. **No comments in `src/` or `ui/src/`.** Name things so the code reads on its own. Shell under `release_manager/` is the exception: its safety gates carry their reasons.
11. **Tests live in `tests/`**, mirroring the source (`tests/unit/<module>.rs`, included with `#[path]`), plus `ui/tests/` and `tests/sidecar/`. Write the test first. Watch a new guard fail with its fix undone. Coverage stays at or above 80% (it is about 93% for Rust and 99% for the UI).
12. **Files hold 200–400 lines, 800 at most.** Split one when your change touches it.
13. **clippy runs with `-D warnings`. No `#[allow]`.**
14. **Commit to `tamagami-hi/dev`, one commit per task**, each compiling and passing on its own. **The commit body is the log:**
    - subject `type(scope): imperative summary`, under about 70 characters;
    - a body of 150–250 words: why, what was verified with real numbers, tradeoffs, and corrections to earlier beliefs;
    - one identity only, `tamagami-hi <193059379+tamagami-hi@users.noreply.github.com>`, with no `Co-Authored-By`, `Signed-off-by` or tool attribution of any kind.
15. **When you fix a bug,** ask whether it is an instance or a class, and sweep for the shape.
16. **Report real numbers:** test counts and coverage re-measured, not remembered.
17. **Update the docs in the same change** (below). A change isn't done until the docs describe it.

## Preparing context

1. **Always, at the start and after every compaction:** this file, [docs/decisions.md](docs/decisions.md) whole, the newest section of [docs/build-log.md](docs/build-log.md), and `git log -15`.
2. **For the task:** the manual that covers it ([docs/README.md](docs/README.md) indexes them). The reference implementation is `~/Learning/1. RUST/blackbox_trage`. Its `src/dhan/dhan_oauth/`, `src/telegram/` and `sidecar/` are where this service's login, notices and browser came from.
3. **Before editing a file:** read it and its tests, and `git log` its path.
4. **Trust the code over the docs.** If a document is wrong, correct it in the same change.

## Updating the docs after a change lands

- **`docs/decisions.md`:** when the operator decides, write it the same day. A decided D-row moves to the decided table with the date and the switch it sets.
- **`docs/build-log.md`:** a new dated section with what was built, what was verified (numbers), and what was corrected.
- **`docs/manual/`:** every changed route, environment variable, command, deployment step or stage.
- **`config/env.example` and `release_manager/stacks/broker_login/.env.example`:** a new variable goes in both, or the export refuses to ship.
- **`docs/README.md`:** index every document added or moved.

## Verification

Run the whole sweep at every boundary:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
node --test tests/sidecar/*.test.mjs
(cd ui && npm run typecheck && npm test && npm run build)
./release_manager/status.sh --do validate
```

For the Postgres contract, set `CBL_TEST_DATABASE_URL` (see [running-and-configuration.md](docs/manual/running-and-configuration.md)).

## Everything else

| Need | Where |
| --- | --- |
| every document, by status | [docs/README.md](docs/README.md) |
| running locally, every environment variable | [docs/manual/running-and-configuration.md](docs/manual/running-and-configuration.md) |
| how a login works, the morning window, notices | [docs/manual/login-flow.md](docs/manual/login-flow.md) |
| the token API contract and a client example for engines | [docs/manual/http-api.md](docs/manual/http-api.md) |
| the VPS, Cloudflare Tunnel, Google OAuth, Access, Dhan setup | [docs/manual/deployment.md](docs/manual/deployment.md) |
| releasing: `status.sh` | [release_manager/README.md](release_manager/README.md) |
| the threat model and phase 6 hardening | [docs/manual/security.md](docs/manual/security.md) |
| the legacy Next.js env store (D7) | [docs/legacy/env-home-nextjs.md](docs/legacy/env-home-nextjs.md) |
