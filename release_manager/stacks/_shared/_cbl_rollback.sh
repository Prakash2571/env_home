#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# _cbl_rollback.sh — the generic VPS-native rollback flow.
#
# Runs ON THE VPS. Restores a previously archived release: its images and the
# compose file that started them, as a matched pair.
#
# Volumes are never touched, and the database is never restored here. The
# previous image runs against the current schema, which is safe only because
# migrations stay backward compatible for one release. Restoring a dump is a
# separate, manual decision (APP_GUIDE.md), because it discards every token and
# audit row written since the dump.
# ─────────────────────────────────────────────────────────────────────────────

cbl_rollback_main() {
    local paths_file="$1"; shift

    local TARGET='' ASSUME_YES=false SKIP_CHECKS=false LIST_ONLY=false
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --to)          TARGET="${2:-}"; shift 2 ;;
            --list)        LIST_ONLY=true; shift ;;
            --yes|-y)      ASSUME_YES=true; shift ;;
            --skip-checks) SKIP_CHECKS=true; shift ;;
            --help|-h)     cbl_rollback_usage; return 0 ;;
            *) printf 'Unknown argument: %s\n' "$1" >&2; cbl_rollback_usage >&2; return 1 ;;
        esac
    done

    require_cmds jq sha256sum gzip find sort
    cbl_load_paths "$paths_file"
    P[paths_file]="$paths_file"

    printf '\n%s═══ broker login rollback · %s ═══%s\n' "$_c_bold" "${P[stack]}" "$_c_rst"

    local current available
    current="$(cbl_current_version)"
    # Newest archive last, by when it was archived.
    available="$(find "${P[rollback_images]}" -mindepth 1 -maxdepth 1 -type d -printf '%T@\t%f\n' 2>/dev/null \
                 | sort -n | cut -f2 || true)"

    if [[ "$LIST_ONLY" == true ]]; then
        printf '\n   current: %s\n\n   available rollback targets (oldest first):\n' "${current:-<none>}"
        if [[ -n "$available" ]]; then
            printf '%s\n' "$available" | sed 's/^/     /'
        else
            printf '     <none>\n'
        fi
        if [[ "${P[db_enabled]}" == true ]]; then
            printf '\n   database dumps (newest last):\n'
            find "${P[db_dump_dir]}" -maxdepth 1 -type f -name '*.dump' -printf '     %f\n' 2>/dev/null \
                | sort | tail -n 5
        fi
        printf '\n'
        return 0
    fi

    [[ -n "$available" ]] || die "no rollback archives under ${P[rollback_images]}"

    if [[ -z "$TARGET" ]]; then
        TARGET="$(printf '%s\n' "$available" | grep -vx -- "${current:-}" | tail -n1 || true)"
        [[ -n "$TARGET" ]] || die "no rollback target other than the running version"
        info "no --to given; selecting $TARGET"
    fi

    [[ "$TARGET" != "$current" ]] || die "$TARGET is already the running version"
    [[ "$TARGET" =~ ^[A-Za-z0-9._-]+$ ]] || die "refusing an unsafe version label: $TARGET"

    local rb="${P[rollback_images]}/$TARGET"
    [[ -d "$rb" ]] || die "no rollback archive for $TARGET at $rb"

    cbl_lock
    cbl_open_log deploy
    cbl_assert_docker

    step "1/6 verify the rollback archive"
    cbl_rollback_verify "$rb"

    step "2/6 confirm"
    if [[ "$ASSUME_YES" != true ]]; then
        if [[ -t 0 ]]; then
            local reply
            printf '\n%s  ➜ Roll %s back to %s? The database stays as it is. [y/N] %s' \
                "$_c_bold" "${current:-<none>}" "$TARGET" "$_c_rst"
            read -r reply || reply=''
            [[ "$reply" == [yY] || "$reply" == [yY][eE][sS] ]] \
                || { warn "aborted by operator"; return 0; }
        else
            die "rollback needs --yes when running non-interactively"
        fi
    fi

    step "3/6 load the archived images"
    local key archive path
    while IFS=$'\t' read -r key archive; do
        path="$rb/$archive"
        [[ -f "$path" ]] || die "rollback archive is missing $archive"
        gzip -dc "$path" | "$(docker_bin)" image load >/dev/null \
            || die "failed to load $archive from the rollback archive"
        info "restored $archive"
    done < <(cbl_images)

    step "4/6 restore the matching compose file"
    if [[ -f "$rb/${P[compose_name]}" ]]; then
        cp "$rb/${P[compose_name]}" "${P[compose_file]}"
        ok "compose file restored from the archive"
    else
        warn "archive has no compose file — keeping the current one"
    fi

    step "5/6 start the restored release"
    CBL_VERSION_FOR_COMPOSE="$TARGET"
    cbl_validate_compose
    compose up -d --remove-orphans || die "failed to start the restored release"
    compose ps

    step "6/6 health gate"
    if [[ "$SKIP_CHECKS" == true ]]; then
        warn "health gate SKIPPED by flag"
    elif ! cbl_health_gate; then
        cbl_capture_app_log "rollback-failed-$TARGET"
        cbl_write_version "$TARGET" "$current" unhealthy
        die "rolled back to $TARGET but it did not come up healthy — manual intervention required"
    fi

    cbl_write_version "$TARGET" "$current" rolled-back
    cbl_update_registry "$TARGET"
    cbl_check_edge

    cbl_summary "Rollback complete" \
        "stack=${P[stack]}" \
        "restored=$TARGET" \
        "was=${current:-<none>}" \
        "log=${CBL_LOG_FILE:-<none>}"
}

cbl_rollback_usage() {
    cat <<'USAGE'
Restore a previously archived release.

  --to VERSION    which release to restore (default: newest archived, not current)
  --list          show the current version, rollback targets and recent dumps
  --yes, -y       skip the confirmation prompt
  --skip-checks   start the release but do not gate on health
  --help, -h      this message

Images and the compose file are restored. Volumes and the database are not.
USAGE
}
