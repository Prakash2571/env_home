# Deployment: the VPS, Cloudflare and Google

Everything needed to put the service on `beonedge` behind a `beonedge.in` subdomain. The release pipeline itself is in [release_manager/README.md](../../release_manager/README.md). This page covers the one-time setup around it.

## The box (verified 2026-10-22)

| Fact | Value |
| --- | --- |
| SSH | `ssh beonedge` (Tailscale `100.122.85.101`, user `beonedge`, in the `docker` group, no passwordless sudo) |
| OS, size | Ubuntu 26.04, 8 cores, 14 GiB RAM; `/srv/dev_stack` on its own 458 GB disk; `/srv/backup` on a 1.8 TB disk |
| Docker | 29.1, compose v5.6 |
| Edge | tunnel **beus** (`34b2f8f4-7b9b-4ff3-869e-27b145f89b75`), `cloudflared` as a systemd service, **locally managed** (verified 2026-10-23): routes live in `/home/beonedge/.cloudflared/config.yml` and the dashboard cannot change them. Every hostname goes to the host nginx on `localhost:80`, which routes by name |
| Neighbours | `boe-*`, `market-data-*`, `aie-*` production stacks; ports 47410–47434 taken. This stack uses **47440** |
| Deploy root | `/srv/dev_stack/CENTRALISED_BROKER_LOGIN` |
| Backup root | `/srv/backup/CENTRALISED_BROKER_LOGIN` |

## Step 0: the two roots (once, needs your sudo password)

`/srv/dev_stack/CENTRALISED_BROKER_LOGIN` was created as `root:root 755`, and `/srv/backup/CENTRALISED_BROKER_LOGIN` does not exist yet. Both must belong to `beonedge`, like every sibling stack:

```bash
ssh -t beonedge sudo install -d -o beonedge -g beonedge -m 750 /srv/dev_stack/CENTRALISED_BROKER_LOGIN /srv/backup/CENTRALISED_BROKER_LOGIN
```

Then run `./release_manager/status.sh` → Provision → Diagnose. It should report only the missing `.env`.

## Step 1: the subdomain and the tunnel (D5)

The route follows every other `beonedge.in` site: tunnel → host nginx on `:80` → the app on `127.0.0.1:47440`. Both files are in [`release_manager/stacks/broker_login/edge/`](../../release_manager/stacks/broker_login/edge/), with `__CBL_HOSTNAME__` where the name goes.

**The name is not in this repository** (D5): the repository is public, and the subdomain is a random one so that scanners don't find it. It lives only in the VPS (`.env`, the tunnel config, the nginx site) and in Cloudflare DNS. Set it in your shell for the commands below, and never commit it:

```bash
CBL_HOST=<subdomain>.beonedge.in
```

**1. The tunnel rule** (no sudo; the file belongs to `beonedge`). Back up `~/.cloudflared/config.yml`, then add the rule from `cloudflared-ingress.yml` (with `__CBL_HOSTNAME__` replaced) **above** the final `- service: http_status:404`, and check it:

```bash
ssh beonedge 'cp ~/.cloudflared/config.yml ~/.cloudflared/config.yml.bak-$(date +%Y%m%d-%H%M%S)'
ssh beonedge "cloudflared tunnel ingress validate && cloudflared tunnel ingress rule https://$CBL_HOST"
```

**2. The DNS record** (no sudo; uses the tunnel's `cert.pem`). It creates the proxied CNAME to `34b2f8f4-….cfargotunnel.com`. Check the reply names `$CBL_HOST` exactly, not the name with another zone appended:

```bash
ssh beonedge "cloudflared tunnel route dns beus $CBL_HOST"
```

**3. The nginx site** (your sudo). It admits only `127.0.0.1` and `::1`, because the app trusts the Access identity header and nothing but cloudflared may bring one:

```bash
sed "s/__CBL_HOSTNAME__/$CBL_HOST/" release_manager/stacks/broker_login/edge/nginx-centralised-broker-login.conf | ssh beonedge 'cat > /tmp/nginx-centralised-broker-login.conf'
ssh -t beonedge 'sudo install -m 644 /tmp/nginx-centralised-broker-login.conf /etc/nginx/sites-available/centralised-broker-login && sudo ln -sfn /etc/nginx/sites-available/centralised-broker-login /etc/nginx/sites-enabled/centralised-broker-login && sudo nginx -t && sudo systemctl reload nginx'
```

**4. Load the tunnel rule** (your sudo). A locally managed tunnel reads its config only at start, so this restarts it. Every `beonedge.in` site drops for a few seconds; pick a quiet moment, never during market hours:

```bash
ssh -t beonedge 'sudo systemctl restart cloudflared && systemctl is-active cloudflared'
```

Until the app is deployed, `https://$CBL_HOST` answers 502 from nginx. **Create the Access applications (step 3) before the first deploy**, or the dashboard would be open to anyone. **Never** publish 47440 any other way.

## Step 2: Google sign-in (Google Cloud Console)

1. <https://console.cloud.google.com> → create or choose a project → **APIs & Services → OAuth consent screen**: User type *External*, app name "Broker Login", your support email. Publishing status can stay *Testing* if you add your own Google accounts as test users.
2. **Credentials → Create credentials → OAuth client ID** → *Web application*.
   - Authorised JavaScript origins: `https://<team-name>.cloudflareaccess.com`
   - Authorised redirect URI: `https://<team-name>.cloudflareaccess.com/cdn-cgi/access/callback`

   `<team-name>` is in Zero Trust → Settings → Custom Pages.
3. Copy the client ID and secret into **Zero Trust → Settings → Authentication → Login methods → Add → Google**, then **Test**.

## Step 3: three Access applications

In **Zero Trust → Access → Applications → Add → Self-hosted**, create these. Access always matches the most specific path, so the order doesn't matter.

| Name | Domain / path | Policy | Why |
| --- | --- | --- | --- |
| Broker Login | `<subdomain>.beonedge.in` (whole host) | **Allow**, Include → Emails: your Google accounts (D6), login method Google | the dashboard and its API |
| Broker Login — Dhan callback | `<subdomain>.beonedge.in/dhan` | **Bypass**, Include → Everyone | Dhan's servers redirect the browser here and cannot sign in. Without this a manual login hangs. The route validates every callback itself. |
| Broker Login — token API | `<subdomain>.beonedge.in/api/v1` | **Service Auth**, Include → Service Token: one per trading engine (D4) | stops crawlers and scanners before they reach the TOTP gate. Each engine sends `CF-Access-Client-Id` and `CF-Access-Client-Secret`. |

If D4 is decided **without** service tokens, make the third application **Bypass** instead. Then the TOTP gate, the lockout and the per-IP limit are the only protection, so add the Cloudflare rate limit in step 5.

Create a service token in **Access → Service Auth → Create Service Token**, one per engine (e.g. `blackbox-centrum`). It is shown once; store it in that engine's `.env`. Revoke it there when an engine is retired. Set `CBL_API_REQUIRE_ACCESS_JWT=true` in this service's `.env`, so the app also refuses a request that somehow arrives without Access.

## Step 4: Dhan

For **each account**, in its Dhan web dashboard → *DhanHQ Trading APIs* → create (or edit) an API app with redirect URL `https://<subdomain>.beonedge.in/dhan/callback`. The API key and secret go into `DHAN_<ALIAS>_API_KEY` and `_API_SECRET`. The account's authenticator seed (from enabling TOTP on Dhan) goes into `DHAN_<ALIAS>_TOTP_SECRET`.

## Step 5: Cloudflare hardening (phase 6, when you're ready)

- **Security → WAF → Rate limiting rules:** `http.request.uri.path eq "/api/v1/token"`, 20 requests per minute per IP, block for 10 minutes.
- **Security → Bots:** Bot Fight Mode on. Access already keeps bots off everything except `/dhan` and, with D4, `/api/v1`.
- **SSL/TLS:** Full. The tunnel handles TLS to Cloudflare, so there is no origin certificate to manage.

## Step 6: `.env` and the first deploy

1. Generate the secrets on your own machine:

   ```bash
   openssl rand -hex 32
   ```

   Use it for `CBL_ENCRYPTION_KEY`, and a separate run for `POSTGRES_PASSWORD`. Generate a base32 seed per account for the token API (D3), for example:

   ```bash
   python3 -c "import base64,os;print(base64.b32encode(os.urandom(20)).decode())"
   ```

2. Write `/srv/dev_stack/CENTRALISED_BROKER_LOGIN/broker_login/.env` from `.env.example`, then `chmod 600` it.
3. `./release_manager/status.sh` → Export → Build bundle, then Ship + Deploy → Ship + deploy.
4. Open the subdomain, sign in with Google, check the cards, and run **Manual login** once per account to prove the callback route works end to end.
5. Put each API seed into the engine that will fetch that account's token, ideally by scanning it into an authenticator app as well, so a person can test with `curl`.

## Backups

The deploy dumps Postgres before every release (the last 14 are kept, in `/srv/backup/…/backup/db`). Back up `CBL_ENCRYPTION_KEY` **somewhere else**: a dump without the key is unreadable, and a key stored beside its dump protects nothing.
