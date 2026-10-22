# Env Home

A small, self-hosted Next.js app for keeping environment variables in named groups. Use the dashboard to edit variables or paste many `NAME=value` lines. Each group has its own read-only access keys, so your backend servers can fetch only the groups they need.

## What it does

- One admin password for the dashboard
- Named groups of variables, with individual editing and bulk paste
- PostgreSQL storage with AES-256-GCM encryption of values before database writes
- One-time display of read-only access keys; only their SHA-256 hashes are stored
- Optional group IP allowlist (IPv4, IPv6, or CIDR)
- JSON fetch endpoint for `curl` and other backend clients

## Requirements

- Node.js 22 or later
- PostgreSQL 15 or later
- HTTPS reverse proxy for internet access

## Setup

1. Create a PostgreSQL database and a user with permission to create tables in it.
2. Copy `.env.example` to `.env`, then set:
   - `DATABASE_URL`: the PostgreSQL connection string
   - `ADMIN_PASSWORD`: your chosen admin password (any non-empty length)
   - `AUTH_SECRET`: a random string of at least 32 characters
   - `ENCRYPTION_KEY`: 32 random bytes encoded as 64 hex characters
3. Generate the last two values with `openssl rand -hex 32` (use a separate result for each).
4. Run:

```bash
npm install
npm run db:setup
npm run build
npm start
```

The app listens on port 3000 by default. Set `PORT` when starting it if you need a different port. For development, use `npm run dev`.

**Keep `ENCRYPTION_KEY` safe and backed up separately from the database.** Without it, stored values cannot be decrypted. Do not commit `.env`.

## Use

1. Sign in using `ADMIN_PASSWORD`.
2. Add a group, for example `payments-prod`.
3. Add variables manually or paste lines like:

```text
DATABASE_URL=postgres://user:pass@db:5432/app
STRIPE_KEY=example
```

4. Click **Save variables**. Pasting only adds values to the editor until you save.
5. Under **Server access**, create a key for each server and copy it immediately. It is shown only once.

Fetch a group:

```bash
curl --fail --silent --show-error \
  -H "Authorization: Bearer YOUR_ACCESS_KEY" \
  https://env.example.com/api/v1/groups/payments-prod
```

The response looks like:

```json
{
  "group": "payments-prod",
  "variables": {
    "DATABASE_URL": "postgres://user:pass@db:5432/app",
    "STRIPE_KEY": "example"
  }
}
```

Treat the response as secret data. Never print it to shared logs. Changing a variable changes the next fetch; running processes must fetch again or restart to see the new value. Revoking a key stops future fetches.

## IP restrictions

A group can have one IP address or CIDR range per line. Empty means any IP that presents a valid key.

IP restrictions work only when a **trusted reverse proxy** removes any client-supplied `X-Real-IP` header and sets it to the actual client IP. Then set `TRUST_PROXY=true` in `.env`. For example, in an Nginx location that proxies to this app:

```nginx
proxy_set_header Host $host;
proxy_set_header X-Forwarded-Proto $scheme;
proxy_set_header X-Real-IP $remote_addr;
proxy_pass http://127.0.0.1:3000;
```

Keep port 3000 private so clients cannot bypass that proxy. If `TRUST_PROXY` is not true, any group with IP rules denies fetches. Access keys remain required even for allowed IPs.

## Security notes

- Put the dashboard and API behind HTTPS. Do not expose PostgreSQL publicly.
- Use a unique access key per server and revoke a key when that server is retired or compromised.
- Back up PostgreSQL and `ENCRYPTION_KEY` separately. Test restoring both.
- The app is designed for a single administrator and a small deployment. It does not include multi-user roles, automated key rotation, or a high-availability control plane.
