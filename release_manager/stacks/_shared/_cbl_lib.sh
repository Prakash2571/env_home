#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# _cbl_lib.sh — shared VPS-native runtime library.
#
# Runs ON THE VPS. Sourced by the native deploy and rollback scripts. Everything
# it does is driven by paths.json, so one implementation serves every stack and
# they cannot drift apart.
#
# `docker`, never `sudo docker`: the deploy user is in the docker group.
#
# THIS BOX IS SHARED. It also runs the boe-*, market-data-* and aie-* services.
# Every docker command here is scoped to this stack's compose project, its own
# container names or its own image tags. Nothing here prunes images, volumes or
# networks globally, and nothing may: `docker system prune` on this host would
# reach another team's rollback images.
# ─────────────────────────────────────────────────────────────────────────────

if [[ -t 1 ]]; then
    _c_rst=$'\033[0m'; _c_bold=$'\033[1m'; _c_dim=$'\033[2m'
    _c_red=$'\033[31m'; _c_grn=$'\033[32m'; _c_ylw=$'\033[33m'; _c_cyn=$'\033[36m'
else
    _c_rst=''; _c_bold=''; _c_dim=''; _c_red=''; _c_grn=''; _c_ylw=''; _c_cyn=''
fi

declare -A P=()
CBL_LOG_FILE=''

step() { printf '\n%s→%s %s\n' "$_c_cyn" "$_c_rst" "$1"; log "STEP $1"; }
ok()   { printf '   %s✓%s %s\n' "$_c_grn" "$_c_rst" "$1"; log "OK $1"; }
warn() { printf '   %s!%s %s\n' "$_c_ylw" "$_c_rst" "$1" >&2; log "WARN $1"; }
info() { printf '     %s%s%s\n' "$_c_dim" "$1" "$_c_rst"; log "INFO $1"; }
die()  { printf '   %s✗%s %s\n' "$_c_red" "$_c_rst" "$1" >&2; log "FAIL $1"; exit 1; }

log() {
    [[ -n "$CBL_LOG_FILE" ]] || return 0
    printf '%s %s\n' "$(date -Is)" "$1" >> "$CBL_LOG_FILE" 2>/dev/null || true
}

require_cmds() {
    local c
    for c in "$@"; do command -v "$c" >/dev/null || die "$c is required on this host"; done
}

# ── contract ────────────────────────────────────────────────────────────────
# cbl_load_paths <paths.json> — read the contract into P[].
#
# Read once into an associative array so a later step cannot re-read a file that
# changed underneath it mid-deploy.
cbl_load_paths() {
    local file="$1" key value
    [[ -f "$file" ]] || die "path contract missing: $file"
    require_cmds jq
    jq empty "$file" 2>/dev/null || die "path contract is not valid JSON: $file"

    [[ "$(jq -r '.schema // 0' "$file")" == "1" ]] || die "unsupported path contract schema"

    while IFS=$'\t' read -r key value; do
        P["$key"]="$value"
    done < <(jq -r '
        {
          stack: .stack, environment: .environment, short: .short,
          image_prefix: .image_prefix,
          root: .vps.root, stack_dir: .vps.stack_dir,
          images_dir: .vps.images_dir,
          compose_file: .vps.compose_file, compose_name: .vps.compose_name,
          env_file: .vps.env_file, env_example: .vps.env_example,
          version_file: .vps.version_file, version_name: .vps.version_name,
          manifest_file: .vps.manifest_file, checksums_file: .vps.checksums_file,
          registry: .vps.registry, data_volume: .vps.data_volume,
          docker: .vps.docker, app_container: .vps.app_container,
          login_container: (.vps.login_container // ""),
          compose_project: .vps.compose_project, lock_file: .vps.lock_file,
          backup_root: .backup.root,
          rollback_root: .backup.rollback_root,
          rollback_images: .backup.rollback_images,
          logs_root: .backup.logs_root,
          deploy_log: .backup.deploy_log, app_log: .backup.app_log,
          db_enabled: (.database.enabled // false | tostring),
          db_container: (.database.container // ""),
          db_dump_dir: (.database.dump_dir // ""),
          db_keep: (.database.keep_dumps // 0 | tostring),
          health_mode: .health.mode,
          health_http_url: (.health.http_url // ""),
          health_settle: (.health.settle_seconds // 20 | tostring),
          health_log_pattern: (.health.log_pattern // ""),
          edge_unit: (.edge.systemd_unit // ""),
          edge_origin: (.edge.origin_url // ""),
          keep_releases: (.retention.keep_releases | tostring)
        } | to_entries[] | [.key, (.value // "")] | @tsv' "$file")

    [[ -n "${P[stack_dir]}" ]] || die "contract has no vps.stack_dir"
    cbl_assert_safe_dir "${P[stack_dir]}"
    cbl_assert_safe_dir "${P[rollback_images]}"
    [[ "${P[db_enabled]}" != true ]] || cbl_assert_safe_dir "${P[db_dump_dir]}"
}

# Refuse a path that could be the filesystem root or escape by traversal. The
# rollback and dump trees get pruned, so this is load-bearing.
cbl_assert_safe_dir() {
    local dir="${1:-}" depth
    [[ -n "$dir" ]]                     || die "refusing an empty path"
    [[ "$dir" == /* ]]                  || die "refusing a relative path: $dir"
    [[ "$dir" != *".."* ]]              || die "refusing a path with ..: $dir"
    [[ "$dir" =~ ^/[A-Za-z0-9_./-]+$ ]] || die "refusing an unsafe path: $dir"
    depth="$(printf '%s' "${dir//[!\/]/}" | wc -c)"
    (( depth >= 3 )) || die "refusing a shallow path: $dir"
}

docker_bin() { printf '%s\n' "${P[docker]:-docker}"; }

cbl_image_tag() { printf '%s-%s:%s\n' "${P[image_prefix]}" "$1" "$2"; }

# compose <args...> — docker compose, always with this stack's project and file.
compose() {
    ( cd "${P[stack_dir]}" && \
      CBL_VERSION="${CBL_VERSION_FOR_COMPOSE:?CBL_VERSION_FOR_COMPOSE not set}" \
      "$(docker_bin)" compose \
        --project-name "${P[compose_project]}" \
        --file "${P[compose_file]}" "$@" )
}

# ── locking ─────────────────────────────────────────────────────────────────
# One lock shared by deploy and rollback, so the two can never interleave.
cbl_lock() {
    require_cmds flock
    exec {CBL_LOCK_FD}>"${P[lock_file]}" \
        || die "cannot open lock file ${P[lock_file]}"
    flock -n "$CBL_LOCK_FD" \
        || die "another deploy or rollback holds ${P[lock_file]}"
    ok "acquired ${P[lock_file]}"
}

# ── logging ─────────────────────────────────────────────────────────────────
cbl_open_log() {
    local kind="$1" dir
    case "$kind" in
        deploy) dir="${P[deploy_log]}" ;;
        *)      dir="${P[app_log]}" ;;
    esac
    mkdir -p "$dir" 2>/dev/null || true
    CBL_LOG_FILE="$dir/$(date -u +%Y%m%dT%H%M%SZ)-$kind.log"
    : > "$CBL_LOG_FILE" 2>/dev/null || CBL_LOG_FILE=''
    [[ -n "$CBL_LOG_FILE" ]] && ok "logging to $CBL_LOG_FILE"
}

# ── preconditions ───────────────────────────────────────────────────────────
cbl_assert_writable() {
    local dir
    for dir in "$@"; do
        [[ -n "$dir" ]] || continue
        mkdir -p "$dir" 2>/dev/null || die "cannot create $dir"
        [[ -w "$dir" ]] || die "not writable: $dir"
    done
}

cbl_assert_backup_tree() {
    cbl_assert_writable "${P[backup_root]}" "${P[rollback_root]}" \
                       "${P[rollback_images]}" "${P[logs_root]}" \
                       "${P[deploy_log]}" "${P[app_log]}"
    if [[ "${P[db_enabled]}" == true ]]; then
        cbl_assert_writable "${P[db_dump_dir]}"
        # Dumps hold every encrypted token and account row. The values are
        # ciphertext, but the directory still should not be world-readable.
        chmod 700 "${P[db_dump_dir]}" 2>/dev/null || warn "could not restrict ${P[db_dump_dir]} to 700"
    fi
    ok "backup and log tree writable"
}

# cbl_assert_space <dir> <need MiB>
cbl_assert_space() {
    local dir="$1" need="$2" avail
    avail="$(df -BM --output=avail "$dir" 2>/dev/null | tail -n1 | tr -dc '0-9')"
    [[ -n "$avail" ]] || { warn "cannot determine free space on $dir"; return 0; }
    (( avail >= need )) || die "insufficient space on $dir: ${avail}MiB free, need ${need}MiB"
    info "$dir has ${avail}MiB free (need ${need}MiB)"
}

# Twice the incoming archive size, so the outgoing release can be archived
# alongside it before the new one is loaded.
cbl_required_space_mib() {
    local total=0
    if [[ -d "${P[images_dir]}" ]]; then
        total="$(du -sm "${P[images_dir]}" 2>/dev/null | cut -f1 || echo 0)"
    fi
    printf '%s\n' "$(( total * 2 + 512 ))"
}

cbl_assert_docker() {
    "$(docker_bin)" info >/dev/null 2>&1 || die "docker is not usable by $(id -un)"
    "$(docker_bin)" compose version >/dev/null 2>&1 || die "docker compose plugin missing"
    ok "docker and compose available"
}

# The app will not start usefully without credentials, so a missing env file is
# worth failing fast on. That is the ONLY thing this checks.
#
# .env belongs entirely to the operator. Nothing in this pipeline reads its
# contents, writes it, changes its mode, or deletes it: deploy.sh excludes it
# from rsync and runs without --delete, and no script greps it. Only Docker reads
# it, via env_file in the compose file, at container start.
cbl_assert_env() {
    [[ -e "${P[env_file]}" ]] \
        || die "missing ${P[env_file]} — place it yourself, then redeploy"
    ok "env file present (contents not inspected)"
}

cbl_validate_compose() {
    compose config --quiet >/dev/null 2>&1 || {
        compose config --quiet 2>&1 | tail -20 >&2
        die "compose file is not valid for this release"
    }
    ok "compose file validates"
}

# ── version bookkeeping ─────────────────────────────────────────────────────
cbl_current_version() {
    [[ -f "${P[version_file]}" ]] || return 0
    jq -r '.version // empty' "${P[version_file]}" 2>/dev/null || true
}

cbl_incoming_version() {
    [[ -f "${P[manifest_file]}" ]] || die "manifest.json missing — has deploy.sh shipped a bundle?"
    local v; v="$(jq -r '.version // empty' "${P[manifest_file]}" 2>/dev/null || true)"
    [[ -n "$v" ]] || die "manifest.json has no version"
    printf '%s\n' "$v"
}

# cbl_write_version <version> <previous> <status> [attempted]
#
# A failed deploy must not rewrite history: .version keeps pointing at what is
# actually running and the failed release is recorded separately, so a retry is
# never told the failed version is already deployed.
cbl_write_version() {
    local version="$1" previous="$2" status="$3" attempted="${4:-}" tmp
    tmp="$(mktemp "${P[version_file]}.XXXXXX")" || die "cannot write version file"
    jq -n --arg v "$version" --arg p "$previous" --arg s "$status" \
          --arg a "$attempted" --arg at "$(date -Is)" --arg stack "${P[stack]}" \
        '{stack: $stack, version: $v, previous: $p, status: $s,
          last_attempted: $a, updated_at: $at}' > "$tmp" \
        || die "cannot serialise version file"
    mv "$tmp" "${P[version_file]}"
    ok "recorded $status: ${version:-<none>}"
}

cbl_update_registry() {
    local version="$1" tmp
    [[ -n "${P[registry]}" ]] || return 0
    [[ -s "${P[registry]}" ]] || printf '{}\n' > "${P[registry]}" 2>/dev/null || return 0
    tmp="$(mktemp "${P[registry]}.XXXXXX")" || return 0
    if jq --arg stack "${P[stack]}" --arg v "$version" --arg at "$(date -Is)" \
          '.[$stack] = {version: $v, deployed_at: $at}' "${P[registry]}" > "$tmp"; then
        mv "$tmp" "${P[registry]}"
    else
        rm -f "$tmp"
    fi
}

# ── images ──────────────────────────────────────────────────────────────────
# cbl_images — one "key<TAB>archive" line per declared image.
cbl_images() {
    jq -r '.images[] | [.key, .archive] | @tsv' "${P[paths_file]:-${P[stack_dir]}/paths.json}"
}

cbl_verify_checksums() {
    [[ -f "${P[checksums_file]}" ]] || die "checksums.sha256 missing"
    ( cd "${P[stack_dir]}" && sha256sum -c --quiet "${P[checksums_file]}" ) \
        || die "checksum verification failed — the upload is corrupt, re-ship"
    ok "checksums verified on arrival"
}

# cbl_archive_current_images <dest> <version>
#
# Saves the running images and the compose file that started them, so a rollback
# restores a matched pair rather than an old image under a new compose.
cbl_archive_current_images() {
    local dest="$1" version="$2" key archive tag
    cbl_assert_writable "$dest"
    while IFS=$'\t' read -r key archive; do
        tag="$(cbl_image_tag "$key" "$version")"
        if ! "$(docker_bin)" image inspect "$tag" >/dev/null 2>&1; then
            warn "running image $tag not present locally — cannot archive it"
            continue
        fi
        if [[ -f "$dest/$archive" ]]; then
            info "already archived: $archive"
            continue
        fi
        "$(docker_bin)" save "$tag" | gzip -n > "$dest/$archive" \
            || die "failed to archive $tag"
        info "archived $archive"
    done < <(cbl_images)
    [[ -f "${P[compose_file]}" ]] && cp "${P[compose_file]}" "$dest/${P[compose_name]}"
    ( cd "$dest" && find . -maxdepth 1 -type f ! -name checksums.sha256 -print0 \
        | sort -z | xargs -0 -r sha256sum > checksums.sha256 ) || true
    ok "outgoing release archived to $dest"
}

cbl_load_images() {
    local key archive path
    while IFS=$'\t' read -r key archive; do
        path="${P[images_dir]}/$archive"
        [[ -f "$path" ]] || die "image archive missing: $path"
        gzip -dc "$path" | "$(docker_bin)" image load >/dev/null \
            || die "failed to load $archive"
        info "loaded $archive"
    done < <(cbl_images)
    ok "images loaded"
}

cbl_assert_images_present() {
    local version="$1" key archive tag
    while IFS=$'\t' read -r key archive; do
        tag="$(cbl_image_tag "$key" "$version")"
        "$(docker_bin)" image inspect "$tag" >/dev/null 2>&1 \
            || die "image $tag is not present after load"
    done < <(cbl_images)
    ok "expected image tags present for $version"
}

cbl_rollback_verify() {
    local dir="$1"
    [[ -d "$dir" ]] || die "no rollback archive at $dir"
    [[ -f "$dir/checksums.sha256" ]] || { warn "rollback archive has no checksums"; return 0; }
    ( cd "$dir" && sha256sum -c --quiet checksums.sha256 ) \
        || die "rollback archive at $dir failed checksum verification"
    ok "rollback archive verified"
}

# ── database ────────────────────────────────────────────────────────────────
# cbl_dump_database <label> — a pg_dump taken before anything changes.
#
# Unlike the reference engine, this app owns state that cannot be re-derived:
# minted broker tokens, account rows, consumer keys. The app migrates its schema
# at start, so the moment the new image starts is the moment a rollback stops
# being a pure image swap. A dump before that moment is the only way back if a
# migration is wrong.
#
# The credentials are read INSIDE the container from its own environment
# ($POSTGRES_USER, $POSTGRES_DB), so this never reads the operator's .env.
#
# First deploy: no database container exists yet, so there is nothing to dump.
cbl_dump_database() {
    local label="$1" dest state
    [[ "${P[db_enabled]}" == true ]] || return 0
    state="$("$(docker_bin)" inspect -f '{{.State.Status}}' "${P[db_container]}" 2>/dev/null || echo missing)"
    if [[ "$state" == missing ]]; then
        info "no ${P[db_container]} yet — first deploy, nothing to dump"
        return 0
    fi
    [[ "$state" == running ]] || die "${P[db_container]} is $state; refusing to deploy without a dump"

    dest="${P[db_dump_dir]}/$(date -u +%Y%m%dT%H%M%SZ)-${label}.dump"
    ( umask 077
      "$(docker_bin)" exec "${P[db_container]}" \
          sh -c 'pg_dump --format=custom --username="$POSTGRES_USER" --dbname="$POSTGRES_DB"' \
          > "$dest" ) || { rm -f -- "$dest"; die "pg_dump failed; nothing was changed"; }
    [[ -s "$dest" ]] || { rm -f -- "$dest"; die "pg_dump produced an empty file; nothing was changed"; }
    ok "database dumped to $dest ($(du -h "$dest" | cut -f1))"
}

cbl_prune_dumps() {
    local keep="${P[db_keep]:-0}" count victim
    [[ "${P[db_enabled]}" == true ]] || return 0
    (( keep > 0 )) || return 0
    cbl_assert_safe_dir "${P[db_dump_dir]}"
    count="$(find "${P[db_dump_dir]}" -maxdepth 1 -type f -name '*.dump' | wc -l)"
    (( count > keep )) || return 0
    while IFS= read -r victim; do
        [[ -n "$victim" ]] || continue
        rm -f -- "${P[db_dump_dir]}/$victim" && info "pruned dump $victim"
    done < <(find "${P[db_dump_dir]}" -maxdepth 1 -type f -name '*.dump' -printf '%f\n' \
             | sort | head -n $(( count - keep )))
}

# ── health ──────────────────────────────────────────────────────────────────
cbl_health_gate() {
    local settle="${P[health_settle]:-20}"
    case "${P[health_mode]}" in
        http) cbl_health_http "${P[health_http_url]}" "$settle" ;;
        *)    cbl_health_container "$settle" ;;
    esac
}

cbl_health_container() {
    local settle="$1" state waited=0 name restarts
    name="${P[app_container]}"

    info "letting the app settle for ${settle}s"
    while (( waited < settle )); do
        state="$("$(docker_bin)" inspect -f '{{.State.Status}}' "$name" 2>/dev/null || echo missing)"
        case "$state" in
            running|missing) : ;;
            *) warn "container $name is $state"; return 1 ;;
        esac
        sleep 1
        waited=$(( waited + 1 ))
    done

    state="$("$(docker_bin)" inspect -f '{{.State.Status}}' "$name" 2>/dev/null || echo missing)"
    [[ "$state" == running ]] || { warn "container $name is $state after ${settle}s"; return 1; }

    restarts="$("$(docker_bin)" inspect -f '{{.RestartCount}}' "$name" 2>/dev/null || echo 0)"
    (( restarts == 0 )) || { warn "container restarted $restarts time(s) during settle"; return 1; }
    ok "container running, no restarts in ${settle}s"

    if [[ -n "${P[health_log_pattern]}" ]]; then
        if "$(docker_bin)" logs --tail 200 "$name" 2>&1 | grep -qF "${P[health_log_pattern]}"; then
            ok "log gate matched: ${P[health_log_pattern]}"
        else
            warn "log gate did NOT match: ${P[health_log_pattern]}"
            return 1
        fi
    fi
}

cbl_health_http() {
    local url="$1" settle="$2" waited=0
    require_cmds curl
    while (( waited < settle )); do
        curl -fsS -m 3 -o /dev/null "$url" 2>/dev/null && { ok "health endpoint responded: $url"; return 0; }
        sleep 2
        waited=$(( waited + 2 ))
    done
    warn "health endpoint did not respond within ${settle}s: $url"
    return 1
}

# The public edge is a Cloudflare Tunnel run by systemd, outside this stack and
# shared with every other site on the box. This only reports on it. A tunnel
# that is down is never a reason to roll the app back, and restarting it from
# here would drop every other hostname it carries.
cbl_check_edge() {
    [[ -n "${P[edge_unit]}" ]] || return 0
    if systemctl is-active --quiet "${P[edge_unit]}" 2>/dev/null; then
        ok "${P[edge_unit]} is active"
    else
        warn "${P[edge_unit]} is not active; the app is up but unreachable from the internet"
    fi
    if [[ -n "${P[edge_origin]}" ]] && command -v curl >/dev/null; then
        if curl -fsS -m 3 -o /dev/null "${P[edge_origin]}" 2>/dev/null; then
            ok "tunnel origin answers on ${P[edge_origin]}"
        else
            warn "tunnel origin did not answer on ${P[edge_origin]} (app health already passed)"
        fi
    fi
}

# The sidecar copies its bind-mounted scripts into its module volume at START,
# so compose sees no change when only the mounted contents change and the
# container keeps serving logins with the previous script. A selector fix would
# ship, verify clean, and silently not apply until the next morning's window.
cbl_restart_login_sidecar() {
    local name="${P[login_container]}"
    [[ -n "$name" ]] || return 0
    if "$(docker_bin)" inspect "$name" >/dev/null 2>&1; then
        "$(docker_bin)" restart "$name" >/dev/null \
            && ok "login sidecar restarted to pick up its scripts"
    fi
}

cbl_capture_app_log() {
    local label="$1" dest
    dest="${P[app_log]}/$(date -u +%Y%m%dT%H%M%SZ)-$label.log"
    mkdir -p "${P[app_log]}" 2>/dev/null || return 0
    "$(docker_bin)" logs --tail 400 "${P[app_container]}" > "$dest" 2>&1 || true
    [[ -s "$dest" ]] && info "container log captured to $dest"
}

# ── retention ───────────────────────────────────────────────────────────────
# cbl_prune_rollbacks <dir> <keep>
#
# Only ever removes immediate subdirectories of a path already proven safe by
# cbl_assert_safe_dir. Sorted by modification time, so a dev label and a stable
# label compare by when they were archived rather than by spelling.
cbl_prune_rollbacks() {
    local dir="$1" keep="$2" count victim
    [[ -d "$dir" ]] || return 0
    cbl_assert_safe_dir "$dir"
    count="$(find "$dir" -mindepth 1 -maxdepth 1 -type d | wc -l)"
    (( count > keep )) || return 0
    while IFS= read -r victim; do
        [[ -n "$victim" ]] || continue
        rm -rf -- "${dir:?}/$victim" && info "pruned rollback $victim"
    done < <(find "$dir" -mindepth 1 -maxdepth 1 -type d -printf '%T@\t%f\n' \
             | sort -n | head -n $(( count - keep )) | cut -f2)
}

# cbl_prune_old_images <keep-version...> — remove this stack's own image tags
# that are neither running nor a rollback target. Scoped to the image prefix,
# so another stack's images on this shared box are never touched.
cbl_prune_old_images() {
    local tag version keepers=" $* "
    while IFS= read -r tag; do
        [[ -n "$tag" ]] || continue
        version="${tag##*:}"
        [[ "$keepers" == *" $version "* ]] && continue
        "$(docker_bin)" image rm "$tag" >/dev/null 2>&1 && info "removed image $tag"
    done < <("$(docker_bin)" image ls --format '{{.Repository}}:{{.Tag}}' \
             | grep -E "^${P[image_prefix]}-[a-z0-9_-]+:" || true)
}

cbl_rollback_versions() {
    find "${P[rollback_images]}" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' 2>/dev/null || true
}

cbl_summary() {
    local title="$1" pair; shift
    printf '\n%s══ %s ══%s\n' "$_c_bold" "$title" "$_c_rst"
    for pair in "$@"; do printf '     %-14s %s\n' "${pair%%=*}" "${pair#*=}"; done
    printf '\n'
    log "SUMMARY $title $*"
}
