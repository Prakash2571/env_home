# CLAUDE.md

Read [README.md](README.md) first: it holds the rules, how to prepare context and how the docs are kept. Where this file and the README differ, the README wins and this file is corrected.

## Where the project stands

- **Built** on `tamagami-hi/dev` (2026-10-22): the Rust service, the dashboard, the sidecar, the image, CI and `release_manager/`. See [docs/build-log.md](docs/build-log.md).
- **Accounts are added on the dashboard** (A13), not in the `.env`. Access JWTs are verified (`src/access.rs`).
- **Deployed** 2026-10-23: `0.1.1-dev.33` runs on `beonedge` behind the random subdomain (on the VPS only, never in this repo). See the build log for what is still open.
- **Open decisions** D1–D9 are in [docs/decisions.md](docs/decisions.md). Never decide one in code.
- The operator often drives sessions from a phone. Use `release_manager/status.sh --do <op>` (never the interactive menu) and keep replies short.

## Task map

| Task | Read |
| --- | --- |
| login engine, scheduler, sidecar | `docs/manual/login-flow.md`, `src/login/`, `src/dhan/`, `sidecar/` |
| Dhan accounts (the Accounts page) | `docs/manual/http-api.md` (account routes), `src/accounts/`, `src/api/accounts.rs`, `ui/src/components/AccountsPage.tsx` |
| Cloudflare Access JWTs | `src/access.rs`, `docs/manual/security.md` |
| token API, dashboard routes | `docs/manual/http-api.md`, `src/api/` |
| dashboard | `ui/src/`, `ui/tests/` |
| deploying, the VPS | `release_manager/README.md`, `docs/manual/deployment.md` |
| security, hardening | `docs/manual/security.md` |
