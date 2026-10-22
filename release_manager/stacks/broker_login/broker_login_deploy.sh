#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# broker_login_deploy.sh — VPS-native deploy entry point.
#
# Runs ON THE VPS, in the stack directory. Invoked either by the operator
# machine (release_manager/deploy.sh --app --beonedge) or by hand:
#
#     ssh beonedge 'cd /srv/dev_stack/CENTRALISED_BROKER_LOGIN/broker_login && ./broker_login_deploy.sh'
#
# All logic is shared. This file declares identity and policy only.
#
# Policy: confirmation required. This service holds live broker sessions that
# trading engines depend on, so a deploy is never silent.
# ─────────────────────────────────────────────────────────────────────────────

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=../_shared/_cbl_lib.sh
source "$HERE/_cbl_lib.sh"
# shellcheck source=../_shared/_cbl_deploy.sh
source "$HERE/_cbl_deploy.sh"

CBL_REQUIRE_CONFIRM=true

cbl_deploy_main "$HERE/paths.json" "$@"
