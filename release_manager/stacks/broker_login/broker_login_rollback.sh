#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# broker_login_rollback.sh — VPS-native rollback entry point.
#
# Runs ON THE VPS, in the stack directory:
#
#     ssh beonedge 'cd /srv/dev_stack/CENTRALISED_BROKER_LOGIN/broker_login && ./broker_login_rollback.sh --list'
#
# All logic is shared. This file declares identity only.
# ─────────────────────────────────────────────────────────────────────────────

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=../_shared/_cbl_lib.sh
source "$HERE/_cbl_lib.sh"
# shellcheck source=../_shared/_cbl_rollback.sh
source "$HERE/_cbl_rollback.sh"

cbl_rollback_main "$HERE/paths.json" "$@"
