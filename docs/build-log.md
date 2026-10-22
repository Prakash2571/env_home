# Build log

**What is built, what was verified, and what is open.** A new dated section after every change lands, newest first. The commit bodies carry the detail; this is the map.

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
