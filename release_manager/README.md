# release_manager

The deployment pipeline for the `broker_login` stack on the home VPS (`beonedge`). It is adapted from `blackbox_trage`'s pipeline, with the single-entry control center of `boe_app`'s.

**Run one script:**

```bash
./release_manager/status.sh
```

`status.sh` shows a dashboard, then a menu: **Provision → Export → Ship + Deploy**. Every operation in the menu also exists as a non-interactive command, which is what an agent or a phone session uses:

```bash
./release_manager/status.sh --status                 # dashboard, then exit
./release_manager/status.sh --diagnose               # readiness; exit 1 on blockers
./release_manager/status.sh --do provision
./release_manager/status.sh --do export
./release_manager/status.sh --do deploy --yes
./release_manager/status.sh --do rollback --list
./release_manager/status.sh --do rollback --latest --yes
./release_manager/status.sh --do reload --yes        # pick up an edited .env
./release_manager/status.sh --do logs
```

Without a terminal, any operation that would prompt needs `--yes`.

## Where things live on the VPS

| What | Path |
| --- | --- |
| Stack directory (compose, scripts, images, `.env`) | `/srv/dev_stack/CENTRALISED_BROKER_LOGIN/broker_login` |
| Rollback archives | `/srv/backup/CENTRALISED_BROKER_LOGIN/backup/rollback/images/<version>` |
| Database dumps (mode 700) | `/srv/backup/CENTRALISED_BROKER_LOGIN/backup/db` |
| Deploy and app logs | `/srv/backup/CENTRALISED_BROKER_LOGIN/logs/{deploy,app}` |
| Compose project and containers | `centralised_broker_login`: `cbl-app`, `cbl-postgres`, `cbl-login` |
| Port (loopback only) | `127.0.0.1:47440`, reached through cloudflared and the host nginx vhost in `stacks/broker_login/edge/` |

The layout follows the sibling stacks on this box (`ALGO_INDEX_ENGINE`, `BOE_APP`): code and state under `/srv/dev_stack/<PROJECT>`, backups on the separate backup disk under `/srv/backup/<PROJECT>`.

[`stacks/broker_login/paths.json`](stacks/broker_login/paths.json) is the **only** authority for these paths. No script derives a remote path. To change a path, edit the contract, then validate it (Provision → Validate) and re-export.

## Before the first deploy

1. **Ownership (once, needs your sudo password).** `/srv/dev_stack/CENTRALISED_BROKER_LOGIN` was created as `root:root`, and `/srv/backup/CENTRALISED_BROKER_LOGIN` does not exist yet. Run:

   ```bash
   ssh -t beonedge sudo install -d -o beonedge -g beonedge -m 750 /srv/dev_stack/CENTRALISED_BROKER_LOGIN /srv/backup/CENTRALISED_BROKER_LOGIN
   ```

   `install -d` creates a missing directory and sets the owner and mode of an existing one, without touching anything inside it. The scripts never run sudo.
2. **Provision:** `status.sh` → Provision → Provision the VPS. This creates the tree and stages `.env.example`.
3. **`.env`:** place it yourself at `/srv/dev_stack/CENTRALISED_BROKER_LOGIN/broker_login/.env` with mode 600. `.env.example` beside it lists every variable. No script reads, writes or removes it.
4. **Diagnose:** `status.sh --diagnose` should report no blocking issues.

## The three stages

```
Export          BUILD   this computer   builds the image, proves it starts, stages a bundle
Ship + Deploy   SHIP    this computer   uploads the bundle over Tailscale, invokes the VPS script
<stack>_deploy  RUN     the VPS         every docker command lives here, under a lock
```

- **The version advances in one place.** Export labels the build from `Cargo.toml`. A clean tree exactly on tag `vX.Y.Z` gives `X.Y.Z`; anything else gives `<next>-dev.N.gSHA[.dirty]`.
- **All docker work happens on the VPS**, in `broker_login_deploy.sh` and `broker_login_rollback.sh`. Given only SSH, the stack directory can be deployed or rolled back by hand.
- **Checksums are verified three times:** locally, after upload, and on arrival inside the VPS script.
- **The image must prove itself before it ships.** With no configuration it must log `configuration incomplete`, log `serving the console from /app/ui/dist`, stay running, and run as a non-root user.

## The deploy, step by step (on the VPS)

Lock → release dir → backup tree → disk space → version reconcile → checksums → docker and `.env` present → confirm → **database dump** → archive the outgoing images → load the new images → `compose up` → restart the login sidecar → **health gate** (`http://127.0.0.1:47440/health`) → record the version → prune → report on cloudflared.

If the health gate fails, the deploy **rolls back the images automatically** to the previous release.

## Database and rollback

This stack owns state that can't be re-derived: tokens, accounts and the audit trail. So:

- **Every deploy dumps Postgres first** (`pg_dump -Fc`; the last 14 dumps are kept). The credentials are read inside the container, never from `.env`.
- **Rollback swaps images only.** The previous image runs against the current schema. That is safe only because **migrations stay backward compatible for one release**: add a column in one release and drop it in a later one.
- **Restoring a dump is manual and deliberate** (see `APP_GUIDE.md`), because it discards every token and audit row written since.

## This box is shared

The VPS also runs the `boe-*`, `market-data-*` and `aie-*` production stacks. Every docker command here is scoped to this compose project, its container names or its own `cbl-*` image tags. Nothing prunes globally. `cloudflared` is only reported on, never restarted, because a restart would drop every other site it carries. Deploys use the Tailscale alias `beonedge`, never the public SSH alias.

## Layout

```
status.sh                the only entry point: dashboard, menu and --do
lib/
  ui.sh                  output helpers
  version.sh             version labels from Cargo.toml
  stacks.sh              stack identity and ssh plumbing (no paths)
  hosts.sh               host identity: which box, which ssh alias (no default host)
  paths.sh               path contract validation and lookup
  remote_state.sh        one read-only SSH round trip for the dashboard
  ops_provision.sh       create the VPS tree; print the sudo fix when blocked
  ops_export.sh          build, accept, save and stage a bundle
  ops_ship.sh            upload and invoke the VPS-native deploy
  ops_operate.sh         rollback, reload, logs, containers, diagnose, resources
stacks/
  _shared/               the VPS-native runtime (_cbl_lib, _cbl_deploy, _cbl_rollback)
  broker_login/          paths.json, compose, entry points, .env.example, APP_GUIDE.md
build/                   staged bundles (gitignored)
state/                   local ship ledger (gitignored)
```

The shell scripts here carry comments on purpose. Each safety gate encodes a reason that is expensive to rediscover during an incident.
