#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# hosts.sh — the host registry.
#
# AUTHORITY BOUNDARY
# This file owns HOST identity only: ids, selector resolution, the ssh alias
# each id maps to, and descriptive metadata. It owns NO paths and NO stack
# identity. A stack says WHAT is deployed; a host says WHERE.
#
# THERE IS DELIBERATELY NO DEFAULT HOST
# Only one box exists today, the home VPS. The selector is required anyway: the
# box also runs unrelated production services (boe-*, market-data-*, aie-*),
# and a second target added later must not inherit a silent default. Adding a
# box is an edit to the table below and to ~/.ssh/config, nowhere else.
#
# The ssh alias is the only value here that becomes ssh argv. It is resolved
# from this table rather than from user input, and cbl_ssh_opts charset-validates
# it again before use.
# ─────────────────────────────────────────────────────────────────────────────

CBL_HOSTS=(beonedge)

CBL_HOST="${CBL_HOST:-}"

# host_attr <host> <attr> — echo one attribute, or return 1.
#
#   alias      ssh config alias; must exist in ~/.ssh/config
#   label      the machine's hostname, used in prompts and output
#   site       the public hostname the Cloudflare Tunnel routes to this box
#   paths      the tracked path contract this box deploys against, a file in
#              stacks/<stack>/ (a filename only: the paths stay in the contract)
#   build      this host's bundle directory under build/
host_attr() {
    local host="$1" attr="$2"
    case "$host" in
        beonedge)
            case "$attr" in
                # Reached over Tailscale (100.x). beonedge-pub is the public
                # fallback in ~/.ssh/config and is deliberately not used here:
                # deploys go over the private network only.
                alias)  printf 'beonedge\n' ;;
                label)  printf 'beonedge-vps\n' ;;
                site)   printf 'a random beonedge.in subdomain (D5; on the VPS only, never in this repo)\n' ;;
                paths)  printf 'paths.json\n' ;;
                build)  printf 'broker_login\n' ;;
                *) return 1 ;;
            esac ;;
        *) return 1 ;;
    esac
}

is_host() {
    local h
    for h in "${CBL_HOSTS[@]}"; do [[ "$h" == "$1" ]] && return 0; done
    return 1
}

# host_selector_p <arg> — true when arg is a host keyword, so entry scripts can
# recognise one without duplicating the alias list in every case statement.
host_selector_p() {
    case "${1:-}" in
        --beonedge|beonedge|--home) return 0 ;;
        *) return 1 ;;
    esac
}

resolve_host() {
    case "${1:-}" in
        --beonedge|beonedge|--home) printf 'beonedge\n' ;;
        *) printf 'Unknown host selector: %s\n' "${1:-<empty>}" >&2
           host_usage >&2
           return 1 ;;
    esac
}

host_usage() {
    local h
    printf 'Expected one of:\n' >&2
    for h in "${CBL_HOSTS[@]}"; do
        printf '  --%-9s %s (%s)\n' \
            "$h" "$(host_attr "$h" label)" "$(host_attr "$h" site)" >&2
    done
}

# host_use <host> — bind the session to one host by setting the alias that
# cbl_ssh reads. Exported so resources.sh inherits it.
host_use() {
    local host="$1" alias
    alias="$(host_attr "$host" alias)" || {
        printf 'host_use: unknown host: %s\n' "$host" >&2
        return 1
    }
    CBL_HOST="$host"
    CBL_SSH_ALIAS="$alias"
    export CBL_HOST CBL_SSH_ALIAS
}

# host_require <host> — fail closed when no host was selected. An operator who
# sets CBL_SSH_ALIAS directly is targeting a box outside this table on purpose,
# which is allowed and says so in the output.
host_require() {
    local host="${1:-}"
    if [[ -n "$host" ]]; then
        host_use "$host" || return 1
        return 0
    fi
    if [[ -n "${CBL_SSH_ALIAS:-}" ]]; then
        CBL_HOST='(CBL_SSH_ALIAS)'
        export CBL_HOST
        return 0
    fi
    printf 'A host is required: there is no default.\n' >&2
    host_usage >&2
    return 1
}

# host_describe — one line naming the box a command is about to act on, so the
# confirmation prompt never says only "the VPS".
host_describe() {
    if [[ "$CBL_HOST" == '(CBL_SSH_ALIAS)' ]]; then
        printf '%s (CBL_SSH_ALIAS override)\n' "$CBL_SSH_ALIAS"
        return 0
    fi
    printf '%s [%s]\n' "$(host_attr "$CBL_HOST" label)" "$CBL_SSH_ALIAS"
}

# host_build_dir <stack> — the bundle directory for the bound host, under build/.
# The CBL_SSH_ALIAS override names no host in the table, so it falls back to the
# stack's own name.
host_build_dir() {
    local stack="${1:-}" name
    if [[ "$CBL_HOST" == '(CBL_SSH_ALIAS)' || -z "$CBL_HOST" ]]; then
        printf '%s\n' "$stack"
        return 0
    fi
    name="$(host_attr "$CBL_HOST" build)" || return 1
    printf '%s\n' "$name"
}
