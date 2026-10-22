#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# _cbl_deploy.sh — the generic VPS-native deployment flow.
#
# Runs ON THE VPS. Sourced by <stack>_deploy.sh, which supplies only identity and
# policy. Everything else is driven by paths.json.
#
# Contract with the operator machine: by the time this runs, deploy.sh has placed
# manifest.json, paths.json, checksums.sha256, the compose file, the sidecar
# scripts and images/*.tar.gz into the stack directory. This script owns every
# docker command from here on; deploy.sh runs none.
# ─────────────────────────────────────────────────────────────────────────────

cbl_deploy_main() {
    local paths_file="$1"; shift

    local ASSUME_YES=false SKIP_CHECKS=false FORCE=false RELOAD=false
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --yes|-y)      ASSUME_YES=true; shift ;;
            --skip-checks) SKIP_CHECKS=true; shift ;;
            --force)       FORCE=true; shift ;;
            --reload)      RELOAD=true; shift ;;
            --help|-h)     cbl_deploy_usage; return 0 ;;
            *) printf 'Unknown argument: %s\n' "$1" >&2; cbl_deploy_usage >&2; return 1 ;;
        esac
    done

    require_cmds jq sha256sum gzip find sort
    cbl_load_paths "$paths_file"
    P[paths_file]="$paths_file"

    if [[ "$RELOAD" == true ]]; then
        cbl_reload_main "$ASSUME_YES"
        return
    fi

    printf '\n%s═══ broker login deploy · %s (%s) ═══%s\n' \
        "$_c_bold" "${P[stack]}" "${P[environment]}" "$_c_rst"

    step "1/13 acquire deployment lock"
    cbl_lock

    step "2/13 verify release directory"
    [[ -d "${P[stack_dir]}" ]]     || die "stack directory missing: ${P[stack_dir]}"
    [[ -f "${P[compose_file]}" ]]  || die "compose file missing: ${P[compose_file]}"
    [[ -f "${P[manifest_file]}" ]] || die "manifest.json missing — ship a bundle first"
    ok "release directory present"

    step "3/13 verify backup and log tree"
    cbl_assert_backup_tree
    cbl_open_log deploy

    step "4/13 check disk space"
    local need; need="$(cbl_required_space_mib)"
    cbl_assert_space "${P[stack_dir]}" "$need"
    cbl_assert_space "${P[backup_root]}" "$need"

    step "5/13 reconcile versions"
    local current incoming
    current="$(cbl_current_version)"
    incoming="$(cbl_incoming_version)"
    info "currently deployed: ${current:-<none>}"
    info "incoming release:   $incoming"
    if [[ -n "$current" && "$current" == "$incoming" ]]; then
        if [[ "$FORCE" == true ]]; then
            warn "$incoming is already deployed — proceeding because --force was given"
        else
            die "$incoming is already deployed (use --force to redeploy)"
        fi
    fi

    step "6/13 verify checksums"
    cbl_verify_checksums

    step "7/13 verify docker and environment"
    cbl_assert_docker
    cbl_assert_env
    CBL_VERSION_FOR_COMPOSE="$incoming"
    cbl_validate_compose

    if [[ "${CBL_REQUIRE_CONFIRM:-false}" == true && "$ASSUME_YES" != true ]]; then
        if [[ -t 0 ]]; then
            local reply
            printf '\n%s  ➜ Deploy %s to %s? [y/N] %s' \
                "$_c_bold" "$incoming" "${P[environment]}" "$_c_rst"
            read -r reply || reply=''
            [[ "$reply" == [yY] || "$reply" == [yY][eE][sS] ]] \
                || { warn "aborted by operator"; return 0; }
        else
            die "this stack needs --yes when running non-interactively"
        fi
    fi

    # Before anything changes. The new image migrates the schema at start, so
    # this is the last moment the database matches the outgoing release.
    step "8/13 dump the database"
    cbl_dump_database "before-$incoming"

    step "9/13 archive the outgoing release"
    if [[ -n "$current" ]]; then
        cbl_archive_current_images "${P[rollback_images]}/$current" "$current"
    else
        info "first deploy — nothing to preserve"
    fi

    step "10/13 load new images"
    cbl_load_images
    cbl_assert_images_present "$incoming"

    step "11/13 start the stack"
    # --remove-orphans is scoped to this compose project, so it removes only a
    # service this stack renamed between releases, never another stack's
    # container on this shared box.
    compose up -d --remove-orphans || cbl_deploy_fail "$current" "$incoming" "compose up failed"
    cbl_restart_login_sidecar
    compose ps

    step "12/13 health gate"
    local healthy=true
    if [[ "$SKIP_CHECKS" == true ]]; then
        warn "health gate SKIPPED by flag — this version is recorded unverified"
    else
        cbl_health_gate || healthy=false
    fi
    if [[ "$healthy" != true ]]; then
        cbl_capture_app_log "failed-$incoming"
        cbl_deploy_fail "$current" "$incoming" "health gate failed"
    fi

    cbl_write_version "$incoming" "$current" active
    cbl_update_registry "$incoming"

    step "13/13 retention"
    # Last, deliberately: a failure above must never destroy a rollback target.
    cbl_prune_rollbacks "${P[rollback_images]}" "${P[keep_releases]}"
    cbl_prune_dumps
    # shellcheck disable=SC2046
    cbl_prune_old_images "$incoming" $(cbl_rollback_versions)

    cbl_check_edge

    cbl_summary "Deployment complete" \
        "stack=${P[stack]}" \
        "environment=${P[environment]}" \
        "version=$incoming" \
        "previous=${current:-<none>}" \
        "project=${P[compose_project]}" \
        "log=${CBL_LOG_FILE:-<none>}"
}

# cbl_reload_main <assume_yes> — recreate the RUNNING release's containers so
# an edited .env takes effect. No images change, no version is written, and no
# dump is taken, because nothing about the schema changes.
cbl_reload_main() {
    local assume_yes="$1" current
    printf '\n%s═══ broker login reload · %s ═══%s\n' "$_c_bold" "${P[stack]}" "$_c_rst"
    cbl_lock
    cbl_open_log deploy
    cbl_assert_docker
    cbl_assert_env
    current="$(cbl_current_version)"
    [[ -n "$current" ]] || die "nothing is deployed yet; there is nothing to reload"
    CBL_VERSION_FOR_COMPOSE="$current"
    cbl_validate_compose
    if [[ "$assume_yes" != true ]]; then
        [[ -t 0 ]] || die "reload needs --yes when running non-interactively"
        local reply
        printf '\n%s  ➜ Recreate the %s containers with the current .env? [y/N] %s' \
            "$_c_bold" "$current" "$_c_rst"
        read -r reply || reply=''
        [[ "$reply" == [yY] || "$reply" == [yY][eE][sS] ]] || { warn "aborted by operator"; return 0; }
    fi
    step "recreating containers for $current"
    compose up -d --force-recreate --remove-orphans || die "compose up failed"
    compose ps
    step "health gate"
    cbl_health_gate || { cbl_capture_app_log "reload-failed-$current"; die "reloaded, but the app is not healthy — check .env"; }
    cbl_check_edge
    cbl_summary "Reload complete" "version=$current" "log=${CBL_LOG_FILE:-<none>}"
}

cbl_deploy_usage() {
    cat <<'USAGE'
Deploy the staged release in this directory.

  --yes, -y       skip the confirmation prompt
  --skip-checks   start the stack but do not gate on health
  --force         redeploy even if this version is already active
  --reload        recreate the running release's containers with the current .env
  --help, -h      this message

manifest.json, paths.json, checksums.sha256, the compose file and
images/*.tar.gz must already be present; the operator machine's deploy.sh
places them here.
USAGE
}

# cbl_deploy_fail <previous> <attempted> <reason>
#
# Marks the release failed and attempts an IMAGE-LEVEL rollback. The database is
# not restored: the previous image must run against whatever schema the failed
# one left, which is why migrations are written to be backward compatible with
# the release before them (add a column first, drop it a release later). The
# dump taken in step 8 is the manual way back if a migration was destructive;
# restoring it automatically would discard every token minted since.
cbl_deploy_fail() {
    local previous="$1" attempted="$2" reason="$3"
    warn "deployment failed: $reason"
    compose logs --tail 40 2>/dev/null || true

    if [[ -z "$previous" ]]; then
        cbl_write_version "" "" failed "$attempted"
        die "deployment failed and there is no previous version to restore"
    fi

    local rb="${P[rollback_images]}/$previous"
    if [[ ! -d "$rb" ]]; then
        cbl_write_version "$previous" "" failed "$attempted"
        die "deployment failed and no rollback archive exists at $rb"
    fi

    step "AUTO-ROLLBACK to $previous"
    cbl_rollback_verify "$rb"

    local key archive path
    while IFS=$'\t' read -r key archive; do
        path="$rb/$archive"
        if [[ ! -f "$path" ]]; then
            cbl_write_version "$previous" "" failed "$attempted"
            die "rollback archive is missing $archive — auto-rollback aborted"
        fi
        if ! gzip -dc "$path" | "$(docker_bin)" image load >/dev/null 2>&1; then
            cbl_write_version "$previous" "" failed "$attempted"
            die "rollback archive $archive would not load — auto-rollback aborted"
        fi
        info "restored $archive"
    done < <(cbl_images)

    [[ -f "$rb/${P[compose_name]}" ]] && cp "$rb/${P[compose_name]}" "${P[compose_file]}"

    CBL_VERSION_FOR_COMPOSE="$previous"
    if compose up -d --remove-orphans >/dev/null 2>&1 && cbl_health_gate; then
        cbl_write_version "$previous" "$attempted" rolled-back
        cbl_update_registry "$previous"
        warn "the database was NOT restored; the pre-deploy dump is in ${P[db_dump_dir]}"
        die "deployment failed ($reason) — automatically rolled back to $previous"
    fi

    cbl_write_version "$previous" "" failed "$attempted"
    die "deployment failed ($reason) AND automatic rollback did not come up healthy — manual intervention required"
}
