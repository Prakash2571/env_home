# Security model

What this service protects, who it protects it from, and where each control lives. Phase 6 (hardening against bots and crawlers) builds on this. Its checklist is at the end.

## What is worth stealing

1. **Broker access tokens.** About 24 hours of full trading on an account. Stored AES-256-GCM encrypted, served only through the TOTP gate.
2. **Broker login secrets:** API key and secret, PIN, the Dhan TOTP seed. Only in the VPS `.env` (mode 600). They never reach the browser, the database or a log, and only the PIN and a live code ever reach the sidecar.
3. **Token API seeds.** Whoever holds one can fetch that account's token. Only in this service's `.env` and in the engine that uses it.
4. **`CBL_ENCRYPTION_KEY`.** It opens every stored token. Back it up apart from the dumps.

## Layers, outside in

| Layer | Control | Where |
| --- | --- | --- |
| Network | the origin listens on `127.0.0.1:47440` only; Postgres is on an internal Docker network; the sidecar publishes no port | `compose.broker_login.yml` |
| Edge | Cloudflare Tunnel; Access with Google for the whole host; Bypass only on `/dhan`; Service Auth on `/api/v1` (D4) | [deployment.md](deployment.md) |
| Crawlers | `robots.txt` disallows everything; every response carries `X-Robots-Tag: noindex…`; Access serves bots a login wall, never the app | `src/api/` |
| Dashboard | needs the Access identity header; optional email allowlist (D6); state changes need a JSON content type (no cross-site form posts); a strict CSP and `frame-ancestors 'none'` | `src/api/admin.rs`, `src/api/mod.rs` |
| Token API | POST only; a TOTP from a seed that is not the Dhan seed; ±1 step; a code accepted once; 5 wrong in 10 min locks for 10 min and alerts Telegram; unknown client IDs get the same reply and lockout (no enumeration); 30 requests/min per IP; optional Access JWT (D4); `no-store`; audited | `src/api/token.rs`, `src/api/guard.rs` |
| Callback | validated before use; accepted only while a login waits; exactly one `tokenId`; 20 attempts a login; the token is exchanged with the account's own secret and must come back with the account's client ID | `src/dhan/callback.rs`, `src/dhan/api.rs` |
| Storage | AES-256-GCM, fresh nonce per write, `alias:client_id` as associated data, so a row moved between accounts fails to open | `src/crypto.rs`, `src/vault.rs` |
| Logs | config problems name variables, never values; Telegram errors never carry the bot token; the dashboard never sees a whole client ID or a token | `src/config.rs`, `src/telegram/`, `src/state.rs` |
| Deploy | `.env` is never shipped, read or deleted; checksums verified 3 times; non-root image (uid 10001, `no-new-privileges`); every docker command scoped to this project | `release_manager/` |

## Known limits, said plainly

- **The Client ID is public.** The TOTP is the only secret the API checks, unless D4 adds service tokens. Six digits with drift means 3 in 1,000,000 per guess. The lockout makes that about 15 guesses per 10 minutes per client ID, and the per-IP limit and Cloudflare cut it further. With D4 on, a guesser must also hold a service token.
- **Access headers are trusted** because only cloudflared reaches the origin. If 47440 were ever published beyond loopback, that trust breaks. Phase 6 adds verification of the `Cf-Access-Jwt-Assertion` signature against the team's certificates.
- **The sidecar endpoint is unauthenticated** and reachable only over the compose network. Don't publish it.
- **A stolen token is valid until it expires.** Dhan offers no revocation here. Rotating the API seed stops future fetches only.

## Phase 6 checklist (after the code base is built)

- [ ] Verify `Cf-Access-Jwt-Assertion` (RS256, team certificates, `aud` = the application's AUD tag) on the dashboard and token routes.
- [ ] Cloudflare rate-limiting rule on `/api/v1/token`; Bot Fight Mode; block non-IN countries on `/api/v1` if every engine is in India.
- [ ] Decide D4. If service tokens: set `CBL_API_REQUIRE_ACCESS_JWT=true`.
- [ ] Decide D6 and set `CBL_ADMIN_EMAILS` as defence in depth behind the Access policy.
- [ ] Alert when `/health` stops answering (the box already runs a blackbox exporter: `boe-ms-blackbox`).
- [ ] Rotate each token API seed on a schedule; record the dates in `docs/decisions.md`.
- [ ] Review Telegram's lockout alerts weekly at first, to learn what normal looks like.
