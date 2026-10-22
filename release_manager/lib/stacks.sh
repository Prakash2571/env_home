#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# stacks.sh — the stack registry.
#
# AUTHORITY BOUNDARY
# This file owns stack IDENTITY only: ids, selector resolution, image naming,
# and non-path metadata (compose filename, container names, retention). It owns
# NO paths. Every deployment, backup, log and image path lives in the stack's
# tracked paths.json contract and is read through lib/paths.sh.
#
# One service per repository, so there is exactly one stack. The registry shape
# is kept so a second stack (a staging copy, say) is an edit to these tables.
# ─────────────────────────────────────────────────────────────────────────────

# Set by host_use in lib/hosts.sh, or by an operator targeting a box that is not
# in the host table. Empty fails closed in cbl_ssh_opts.
CBL_SSH_ALIAS="${CBL_SSH_ALIAS:-}"

CBL_STACKS=(broker_login)

# stack_attr <stack> <attr> — echo one non-path attribute, or return 1.
#
#   env            environment name
#   short          short id used in image tags
#   compose        compose filename inside the stack dir
#   version_file   per-stack version filename
#   deploy         native deploy script filename
#   rollback       native rollback script filename
#   guide          guide filename
#   project        compose project name
#   binary         the binary inside the app image, for the acceptance run
#   accept_log     a line the app must log when it starts without configuration
#   accept_ui      a line proving the built UI is inside the image
stack_attr() {
    local stack="$1" attr="$2"
    case "$stack" in
        broker_login)
            case "$attr" in
                env)          printf 'production\n' ;;
                short)        printf 'app\n' ;;
                compose)      printf 'compose.broker_login.yml\n' ;;
                version_file) printf 'app-version.json\n' ;;
                deploy)       printf 'broker_login_deploy.sh\n' ;;
                rollback)     printf 'broker_login_rollback.sh\n' ;;
                guide)        printf 'APP_GUIDE.md\n' ;;
                project)      printf 'centralised_broker_login\n' ;;
                binary)       printf '/usr/local/bin/broker_login\n' ;;
                accept_log)   printf 'configuration incomplete\n' ;;
                accept_ui)    printf 'serving the console from /app/ui/dist\n' ;;
                *) return 1 ;;
            esac ;;
        *) return 1 ;;
    esac
}

is_stack() {
    local s
    for s in "${CBL_STACKS[@]}"; do [[ "$s" == "$1" ]] && return 0; done
    return 1
}

stack_selector_p() {
    case "${1:-}" in
        --app|app|broker_login) return 0 ;;
        *) return 1 ;;
    esac
}

resolve_stack() {
    case "${1:-}" in
        --app|app|broker_login) printf 'broker_login\n' ;;
        *) printf 'Unknown stack selector: %s\n' "${1:-<empty>}" >&2
           printf 'Expected: --app\n' >&2
           return 1 ;;
    esac
}

# stack_images <stack> — one "key:archive" pair per line.
#
# Only images built from this repository are listed. Postgres and the Playwright
# sidecar are pinned upstream images pulled on the VPS, so they are neither
# built nor shipped, and they are not versioned with the app.
stack_images() {
    case "$1" in
        broker_login) printf 'app:app.tar.gz\n' ;;
        *) return 1 ;;
    esac
}

# stack_image_tag <stack> <key> <version> — the fully qualified local image tag.
# The VPS-side library builds the same name from paths.json (.image_prefix), so
# the two must agree; paths_validate checks it.
stack_image_tag() {
    local stack="$1" key="$2" version="$3"
    is_stack "$stack" || return 1
    printf 'cbl-%s:%s\n' "$key" "$version"
}

# ── ssh plumbing ────────────────────────────────────────────────────────────
# CBL_SSH_ALIAS and CBL_SSH_KEY are operator-controlled and become ssh argv, so
# they are charset-validated first: an alias like "-oProxyCommand=..." would
# otherwise smuggle an extra option.
cbl_ssh_opts() {
    if [[ -z "$CBL_SSH_ALIAS" ]]; then
        printf 'cbl_ssh_opts: no host selected; pass a host selector or set CBL_SSH_ALIAS\n' >&2
        return 1
    fi
    if [[ ! "$CBL_SSH_ALIAS" =~ ^[A-Za-z0-9._-]+$ || "$CBL_SSH_ALIAS" == -* ]]; then
        printf 'cbl_ssh_opts: unsafe CBL_SSH_ALIAS: %s\n' \
            "$(printf '%s' "$CBL_SSH_ALIAS" | LC_ALL=C tr -d '\000-\010\013-\037\177')" >&2
        return 1
    fi
    CBL_SSH_OPTS=(-o BatchMode=yes -o ConnectTimeout=15)
    if [[ -n "${CBL_SSH_KEY:-}" ]]; then
        if [[ ! "$CBL_SSH_KEY" =~ ^[A-Za-z0-9._/~+-]+$ || "$CBL_SSH_KEY" == -* ]]; then
            printf 'cbl_ssh_opts: unsafe CBL_SSH_KEY path\n' >&2
            return 1
        fi
        CBL_SSH_OPTS+=(-i "$CBL_SSH_KEY" -o IdentitiesOnly=yes)
    fi
}

cbl_ssh() {
    cbl_ssh_opts || return 1
    ssh "${CBL_SSH_OPTS[@]}" "$CBL_SSH_ALIAS" "$@"
}

# assert_safe_remote_dir <path> — refuse a path that could be the filesystem
# root or escape by traversal, so a later rm -rf inside it cannot become
# catastrophic. Depth >= 3 means a truncated variable cannot resolve to a
# top-level directory.
assert_safe_remote_dir() {
    local dir="${1:-}" shown depth
    shown="$(printf '%s' "$dir" | LC_ALL=C tr -d '\000-\010\013-\037\177')"
    [[ -n "$dir" ]]                     || { printf 'Refusing empty remote dir\n' >&2; return 1; }
    [[ "$dir" != *".."* ]]              || { printf 'Refusing remote dir with ..: %s\n' "$shown" >&2; return 1; }
    [[ "$dir" =~ ^/[A-Za-z0-9_./-]+$ ]] || { printf 'Refusing unsafe remote dir: %s\n' "$shown" >&2; return 1; }
    depth="$(printf '%s' "${dir//[!\/]/}" | wc -c)"
    (( depth >= 3 )) || { printf 'Refusing shallow remote dir: %s\n' "$shown" >&2; return 1; }
}
