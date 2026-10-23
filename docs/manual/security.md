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
| Network | the origin listens on `127.0.0.1:47440` only; the host nginx vhost in front of it admits only loopback (cloudflared), so a forged Access header from the LAN never arrives; Postgres is on an internal Docker network; the sidecar publishes no port | `compose.broker_login.yml`, `edge/nginx-centralised-broker-login.conf` |
| Edge | Cloudflare Tunnel; Access with Google for the whole host; Bypass only on `/dhan`; Service Auth on `/api/v1` (D4) | [deployment.md](deployment.md) |
| Crawlers | `robots.txt` disallows everything; every response carries `X-Robots-Tag: noindex…`; Access serves bots a login wall, never the app | `src/api/` |
| Dashboard | needs the Access identity header; optional email allowlist (D6); state changes need a JSON content type (no cross-site form posts); a strict CSP and `frame-ancestors 'none'` | `src/api/admin.rs`, `src/api/mod.rs` |
| Token API | POST only; a TOTP from a seed that is not the Dhan seed; ±1 step; a code accepted once; **5 wrong in 10 min locks that (client ID, source) for 10 min**, so a guesser cannot lock the engine out from elsewhere; **30 wrong in 10 min from all sources locks the client** (and the shared seed, if D3 is decided that way); unknown client IDs run the same path against a random per-process decoy seed (no enumeration); 30 requests/min per IP, IPv6 grouped by /64; every guard map hard-capped and failing closed; lockout alerts throttled (1 per client per hour, 10 per hour overall); optional Access header check (D4); `no-store`; audited | `src/api/token.rs`, `src/api/guard.rs` |
| Callback | validated before use; accepted only while a login waits; exactly one `tokenId`; 5 attempts per source and 200 per login; a repeated `tokenId` costs no second exchange; a rejected exchange never ends the login early; the token must come back with the account's client ID | `src/dhan/callback.rs`, `src/dhan/api.rs`, `src/login/mod.rs` |
| Storage | AES-256-GCM, fresh nonce per write, `alias:client_id:obtained_at:expires_at` as associated data, so a row moved between accounts or with an edited validity fails to open; Postgres receives only its own three variables, never the `.env`; audit and attempts pruned after 90 days, tokens after 30 | `src/crypto.rs`, `src/vault.rs`, `src/store/`, compose |
| Logs | config problems name variables, never values; secret-bearing types have redacting `Debug`; Telegram errors never carry the bot token; sidecar page text is clipped to 240 characters with URL queries removed; the dashboard never sees a whole client ID (the audit rows included) or a token | `src/config.rs`, `src/dhan/`, `src/telegram/`, `src/api/mod.rs` |
| Sidecar | drives only `https://auth.dhan.co` (`CONSENT_HOSTS`); checks the redirect, PIN and code before a browser opens; no published port | `sidecar/guard.mjs` |
| Deploy | `.env` is never shipped, read or deleted; checksums verified 3 times; every contract path and name charset-checked before it reaches a remote command; non-root image (uid 10001), `no-new-privileges`, all capabilities dropped, read-only root filesystem; log directories 750, dumps 700; every docker command scoped to this project; `cargo audit` and `npm audit` in CI | `release_manager/`, compose, CI |

## Known limits, said plainly

- **The Client ID is public.** The TOTP is the only secret the API checks unless D4 adds service tokens. Six digits with ±1 step drift means 3 in 1,000,000 per guess. One source gets 5 guesses per 10 minutes and all sources together get 30, so a guesser needs on the order of 200 years at the global budget. Cloudflare's rate limit and, with D4, a service token cut it further. The price of the global budget: an attacker spending all 30 guesses locks the client for 10 minutes for everyone, including the engine. That is the deliberate trade against guessing; D4 removes it.
- **Access headers are trusted, not verified.** Only cloudflared reaches the origin from outside, but any local process or host-network container on this shared VPS can call `127.0.0.1:47440` and forge `Cf-Access-Authenticated-User-Email` or `Cf-Connecting-IP`. `CBL_API_REQUIRE_ACCESS_JWT` checks only that the JWT header is **present**. Phase 6 verifies its signature; until then the dashboard labels it "Access header + TOTP", not "service token".
- **The sidecar endpoint is unauthenticated** and reachable only over the compose network. Don't publish it.
- **Replay state is in memory.** A restart forgets which code step was last accepted, so a code captured in the same 30 s step could be replayed once across a restart.
- **The sidecar installs Playwright at first start** (`npm install playwright@1.63.0` into a volume) rather than from `sidecar/package-lock.json`, as in the reference. The version is pinned; the transitive tree is not.
- **A stolen token is valid until it expires.** Dhan offers no revocation here. Rotating the API seed stops future fetches only.

## Phase 6 checklist (after the code base is built)

- [ ] Verify `Cf-Access-Jwt-Assertion` (this also closes the local-process header forgery above) (RS256, team certificates, `aud` = the application's AUD tag) on the dashboard and token routes.
- [ ] Cloudflare rate-limiting rule on `/api/v1/token`; Bot Fight Mode; block non-IN countries on `/api/v1` if every engine is in India.
- [x] Decide D4: service tokens (A12). Set `CBL_API_REQUIRE_ACCESS_JWT=true` in the VPS `.env`.
- [x] Decide D6: only `beonedgellp@gmail.com` (A11). Set `CBL_ADMIN_EMAILS` to it as defence in depth.
- [ ] Alert when `/health` stops answering (the box already runs a blackbox exporter: `boe-ms-blackbox`).
- [ ] Rotate each token API seed on a schedule; record the dates in `docs/decisions.md`.
- [ ] Review Telegram's lockout alerts weekly at first, to learn what normal looks like.
