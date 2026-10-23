# The login flow

How an account gets its token each morning, and what each stage on the dashboard means. Code: `src/login/`, `src/dhan/`, `sidecar/`.

## The six stages

| # | Stage | What happens | Fails when |
| --- | --- | --- | --- |
| 1 | consent | `POST auth.dhan.co/app/generate-consent` with the account's API key and secret, giving a one-time consent URL. Counted against Dhan's cap of **25 a day** per account, and the count is checked before Dhan is called. | wrong API key or secret, the day's cap spent, Dhan down |
| 2 | browser | **Automated:** the Playwright sidecar opens the consent URL and fills the Dhan login ID, the PIN and a fresh TOTP from the account's Dhan TOTP secret, all entered on the Accounts page. A code is sent only with 25 s or more of life left, and never twice. **Manual:** skipped; the card shows the consent link for a person to open. | a wrong PIN or code, a lockout, an SMS OTP prompt, a changed page |
| 3 | callback | Dhan redirects to `DHAN_REDIRECT_URL` with a `tokenId`. Automated: the sidecar catches it in the browser. Manual: the public `/dhan/callback` route receives it and validates it before use. | nobody finishes within 5 minutes (manual) |
| 4 | token exchange | `GET auth.dhan.co/app/consumeApp-consent` gives the access token and its expiry. The returned client ID must equal the account's. | a different client ID, an expired session |
| 5 | stored encrypted | AES-256-GCM under `CBL_ENCRYPTION_KEY`, bound to `alias:client_id`, written to Postgres | the database unreachable |
| 6 | notified | a Telegram notice is queued; skipped if Telegram is not configured | — |

Every attempt is written to `login_attempts`, whether it succeeded or failed, so the day's consent count and the dashboard's attempts table survive restarts.

## The morning window

- Default **08:00–08:30 IST, Monday to Friday** (`CBL_LOGIN_WINDOW`, `CBL_LOGIN_DAYS`). Dhan tokens last about 24 hours, so each account logs in once a morning.
- Inside the window, every account without a token **minted today** is logged in, **one after another**: the sidecar holds one browser at a time.
- A failed account is retried every 60 s, **at most 5 times** a day. After the fifth failure one Telegram notice asks for a manual login.
- A **hopeless** failure stops that account until tomorrow: an SMS OTP prompt (only a person can read it), a lockout ("too many attempts", "locked", "blocked"), or the consent cap spent. The card keeps the consent link so a person can finish.
- When the window closes, **one summary** goes to Telegram naming any account without a token. After a restart later in the day, the summary is sent only if an account is missing.
- The dashboard can run a login **at any hour**, automated or manual, for one account or all.
- An **automated** login from the dashboard is tried **up to 5 times**, 60 s apart (so each try gets a fresh TOTP step), and stops at the first success or at a hopeless failure. A second request for an account that is still retrying is ignored, so two clicks cannot spend ten consents. Each try spends one of the day's 25 consents.
- A **manual** login is tried once: it waits for a person.

## Notices

Telegram (`TELEGRAM_BOT_TOKEN`, `TELEGRAM_CHAT_ID`; D8 open) receives:

- service started;
- each account logged in (path, client hint, how long the token is valid);
- a failed login, with the stage, Dhan's own error, the try number and the consents left today. A run of retries sends **one** notice, after its last try (or at once if it cannot pass), asking for a manual login; the tries before it appear only in the dashboard's activity log;
- the morning summary;
- a token API lockout (client hint, whether it is a known account, source IP).

Messages are HTML with every value escaped, split on line breaks under Telegram's limit, and paced to 20 a minute. `retry_after` is honoured, and a message falls back to plain text if Telegram refuses the markup. An error message never includes the bot token.

## When the broker changes its page

The sidecar finds each box by the text around it (`sidecar/prompt.mjs`), not by fixed selectors. It refuses to type into a box it can't name, and it never sends the same value twice. If Dhan changes the page so that it stops working, see `release_manager/stacks/broker_login/APP_GUIDE.md` for the fix-without-a-release path.

## Where the accounts come from

Accounts are added on the dashboard's **Accounts** page, not in the `.env`. Each is sealed (AES-256-GCM under `CBL_ENCRYPTION_KEY`, bound to its alias and client ID) in the `broker_accounts` table and logged in in the order it was added. At start the service opens every stored account, retrying while Postgres is not up yet; a row that will not open, for example after the key changed, is skipped and reported in the activity log. An added account joins the next scheduled login without a restart; a removed one is dropped at once, but never in the middle of its own login.
