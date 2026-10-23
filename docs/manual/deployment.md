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

## Step 2: Google sign-in

**Already in place on this account (verified 2026-10-23):** Zero Trust → Integrations → Identity providers lists **Google**, used by the existing console. Nothing to do here. For a new account, the steps were:

1. <https://console.cloud.google.com> → **APIs & Services → OAuth consent screen**: *External*, your support email; *Testing* is fine with your own accounts as test users.
2. **Credentials → Create credentials → OAuth client ID** → *Web application*, with origin `https://<team-name>.cloudflareaccess.com` and redirect URI `https://<team-name>.cloudflareaccess.com/cdn-cgi/access/callback`.
3. Zero Trust → Integrations → Identity providers → **Add → Google** with that client ID and secret, then **Test**.

## Step 3: three Access applications (done 2026-10-23)

Zero Trust → Access controls → Applications → **Create new application → Self-hosted and private**. Access matches the most specific path first, so the order doesn't matter. The three apps reuse the account's reusable policies (Access controls → Policies):

| App | Destination | Policy | Sign-in |
| --- | --- | --- | --- |
| `broker-login` | `<subdomain>.beonedge.in`, whole host | **only me** (Allow, `beonedgellp@gmail.com`; A11) | Google only, *instant authentication* on, 24 h session |
| `broker-login-dhan` | `<subdomain>.beonedge.in/dhan` | **dhan-bypass** (Bypass, Everyone) | none: Dhan's redirect cannot sign in, and the route validates every callback itself |
| `broker-login-api` | `<subdomain>.beonedge.in/api/v1` | **broker-engines** (Service Auth, any service token for now; A12) | `CF-Access-Client-Id` + `CF-Access-Client-Secret` from the engine |

Checked from outside: `/` and `/health` redirect to the Access login, `/dhan/callback` reaches nginx (502 until deployed), `/api/v1/token` without a valid service token gets 403 from Cloudflare.

**Per engine (phase 7):** Access controls → Service credentials → **Create service token** (e.g. `blackbox-trage`). Cloudflare shows the secret once; put it straight into that engine's `.env`, never here. Then change **broker-engines** from *Any Access Service Token* to that token, and revoke the token when the engine is retired. Set `CBL_API_REQUIRE_ACCESS_JWT=true` in this service's `.env`.

**Monitoring note:** `/health` is behind Access too. An outside monitor needs its own service token, or a small Bypass app on `/health` (it reveals only `ok`/`unconfigured` and a count). The deploy's health gate uses loopback and is unaffected.

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
