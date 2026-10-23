# broker_login on the VPS

This guide is shipped with every release into `/srv/dev_stack/CENTRALISED_BROKER_LOGIN/broker_login`. Everything below works with nothing but SSH.

```bash
ssh beonedge
cd /srv/dev_stack/CENTRALISED_BROKER_LOGIN/broker_login
```

## What runs

| Container | Image | Reachable from |
| --- | --- | --- |
| `cbl-app` | `cbl-app:<version>` (shipped) | `127.0.0.1:47440` on the host; cloudflared → host nginx (`edge/`) → here |
| `cbl-postgres` | `postgres:17-alpine` (pulled) | `cbl-app` only, over the internal `datastore` network |
| `cbl-login` | `mcr.microsoft.com/playwright:v1.63.0-noble` (pulled) | `cbl-app` only, as `http://login:8090/login` |

## Everyday commands

```bash
./broker_login_deploy.sh                 # deploy the staged release (asks first)
./broker_login_deploy.sh --reload        # recreate containers after editing .env
./broker_login_rollback.sh --list        # releases and recent database dumps
./broker_login_rollback.sh               # back to the newest archived release
./broker_login_rollback.sh --to <ver>    # back to a specific release
cat app-version.json                     # what is live, and what was before it
docker logs --tail 100 cbl-app
curl -fsS http://127.0.0.1:47440/health
```

## Fixing a broken broker login page without a release

The sidecar's scripts are bind-mounted from `./sidecar`. If the broker changes its login page and a selector breaks:

1. Edit the script in the repository and ship (`status.sh` → Ship only).
2. Or, in an emergency, edit `./sidecar/login.mjs` here and run `docker restart cbl-login`. Copy the fix back into the repository afterwards, or the next deploy undoes it.

## Restoring a database dump (manual, deliberate)

A restore **discards every token, login attempt and audit row written since the dump**. Do it only when a migration has damaged data and rolling back the images is not enough.

```bash
ls -1 /srv/backup/CENTRALISED_BROKER_LOGIN/backup/db/            # pick the dump
docker stop cbl-app
docker exec -i cbl-postgres sh -c 'pg_restore --clean --if-exists --no-owner \
    --username="$POSTGRES_USER" --dbname="$POSTGRES_DB"' \
    < /srv/backup/CENTRALISED_BROKER_LOGIN/backup/db/<dump>.dump
./broker_login_rollback.sh --to <the version the dump was taken before>
```

Tokens minted after the dump are gone. Run a fresh login from the UI for each account afterwards.

## Keys you must never lose

`CBL_ENCRYPTION_KEY` in `.env` encrypts every stored token. Back it up **apart from** the database dumps. A dump without the key is unreadable, and a key stored beside its dump protects nothing.
