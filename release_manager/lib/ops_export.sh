#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# ops_export.sh — BUILD stage. Runs ONLY on this computer, never on the VPS.
#
# Run from status.sh (Export menu, or --do export). Responsibilities:
#   1. Determine the version label — the only place that does.
#   2. Build the app image from the working tree and prove it starts.
#   3. docker save | gzip it into a versioned bundle under build/<host>/.
#   4. Copy the VPS-native scripts, compose file, sidecar scripts, paths.json
#      and .env.example into the bundle, so it is everything the VPS needs.
#   5. Write manifest.json and checksums.sha256.
#
# Explicitly NOT this module's job: touching the VPS, running compose anywhere,
# or any git write.
# ─────────────────────────────────────────────────────────────────────────────

# op_export <paths.json> <stack> [--skip-build] [--keep N]
op_export() {
    local paths_file="$1" stack="$2"; shift 2
    local skip_build=false keep=3
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --skip-build) skip_build=true; shift ;;
            --keep)
                [[ "${2:-}" =~ ^[0-9]+$ && "$2" -ge 1 ]] || { err "--keep needs a positive integer"; exit 1; }
                keep="$2"; shift 2 ;;
            *) err "unknown export option: $1"; exit 1 ;;
        esac
    done

    local c
    for c in docker jq gzip sha256sum git; do
        command -v "$c" >/dev/null || { err "$c is required"; exit 1; }
    done

    # ── version label ───────────────────────────────────────────────────────
    local canonical version kind
    [[ -f "$ROOT_DIR/Cargo.toml" ]] || { err "no Cargo.toml at the repository root"; exit 1; }
    canonical="$(cargo_version "$ROOT_DIR/Cargo.toml")" || exit 1
    assert_semver "$canonical" || exit 1
    if on_exact_release_tag "$ROOT_DIR" "$canonical"; then
        version="$canonical"; kind=stable
    else
        version="$(dev_version "$(bump_version "$canonical" patch)" "$ROOT_DIR")"; kind=dev
    fi

    local stamp build_name bundle git_sha git_branch git_dirty=false
    stamp="$(date -u +%Y%m%dT%H%M%SZ)"
    build_name="$(host_build_dir "$stack")" || exit 1
    bundle="$BUILD_DIR/$build_name/${version}-${stamp}"
    git_sha="$(git -C "$ROOT_DIR" rev-parse HEAD 2>/dev/null || echo unknown)"
    git_branch="$(git -C "$ROOT_DIR" symbolic-ref --short -q HEAD 2>/dev/null || echo detached)"
    git_dirty "$ROOT_DIR" && git_dirty=true

    banner "EXPORT · $stack"
    field "environment" "$(stack_attr "$stack" env)"
    field "version"     "$version"
    field "kind"        "$kind"
    field "commit"      "${git_sha:0:9}$([[ "$git_dirty" == true ]] && printf ' (dirty)')"
    field "bundle"      "${bundle#"$ROOT_DIR"/}"

    mkdir -p "$bundle/images"
    # A bundle is only meaningful once manifest.json exists. If this aborts
    # before that, the partial directory goes, so the newest-bundle selection
    # can never pick up a half-built release.
    EXPORT_BUNDLE="$bundle"; EXPORT_COMPLETE=false
    trap 'if [[ "$EXPORT_COMPLETE" != true && -d "$EXPORT_BUNDLE" ]]; then rm -rf -- "$EXPORT_BUNDLE"; printf "   removed incomplete bundle\n" >&2; fi' EXIT

    # ── build ───────────────────────────────────────────────────────────────
    local tag
    tag="$(stack_image_tag "$stack" app "$version")"
    if [[ "$skip_build" == true ]]; then
        section "BUILD IMAGE" "skipped by --skip-build"
        docker image inspect "$tag" >/dev/null 2>&1 \
            || { err "image not present: $tag (build it first)"; exit 1; }
    else
        section "BUILD IMAGE" "$tag"
        export_assert_build_inputs
        DOCKER_BUILDKIT=1 docker build -t "$tag" "$ROOT_DIR" || { err "docker build failed"; exit 1; }
        ok "image built"
    fi

    export_accept_image "$stack" "$tag"

    # ── save ────────────────────────────────────────────────────────────────
    section "SAVE IMAGE"
    local key archive images_json='{}' sha
    while IFS=: read -r key archive; do
        [[ -n "$key" ]] || continue
        tag="$(stack_image_tag "$stack" "$key" "$version")"
        step "saving $tag"
        # gzip -n omits the timestamp so identical input yields an identical
        # archive, and the checksum means something across rebuilds.
        docker save "$tag" | gzip -n > "$bundle/images/$archive"
        sha="$(sha256sum "$bundle/images/$archive" | cut -d' ' -f1)"
        images_json="$(jq --arg k "$key" --arg tag "$tag" --arg a "images/$archive" --arg s "$sha" \
            '.[$k] = {tag: $tag, archive: $a, sha256: $s}' <<<"$images_json")"
        ok "$archive  $(du -h "$bundle/images/$archive" | cut -f1)"
    done < <(stack_images "$stack")

    # ── stage ───────────────────────────────────────────────────────────────
    section "STAGE VPS ARTIFACTS"
    export_stage_files "$stack" "$paths_file" "$bundle"

    # ── manifest ────────────────────────────────────────────────────────────
    section "MANIFEST"
    local compose_name
    compose_name="$(stack_attr "$stack" compose)"
    jq -n \
        --arg version "$version" --arg kind "$kind" --arg stack "$stack" \
        --arg host "$CBL_HOST" --arg environment "$(stack_attr "$stack" env)" \
        --arg created_at "$stamp" --arg git_sha "$git_sha" --arg git_branch "$git_branch" \
        --argjson git_dirty "$git_dirty" \
        --arg compose_file "$compose_name" \
        --arg compose_sha "$(sha256sum "$bundle/$compose_name" | cut -d' ' -f1)" \
        --arg paths_sha "$(sha256sum "$bundle/paths.json" | cut -d' ' -f1)" \
        --argjson images "$images_json" \
        '{version: $version, kind: $kind, stack: $stack, host: $host,
          environment: $environment, created_at: $created_at,
          git_sha: $git_sha, git_branch: $git_branch, git_dirty: $git_dirty,
          compose: {file: $compose_file, sha256: $compose_sha},
          paths: {file: "paths.json", sha256: $paths_sha},
          images: $images}' > "$bundle/manifest.json"
    jq empty "$bundle/manifest.json" || { err "generated an invalid manifest"; exit 1; }

    # A flat list too, so sha256sum -c works on the VPS. Every path resolves
    # relative to the stack directory, where both verifiers run.
    ( cd "$bundle" && find . -type f ! -name 'checksums.sha256' -print0 \
        | sort -z | xargs -0 -r sha256sum > checksums.sha256 )
    EXPORT_COMPLETE=true
    ok "manifest.json and checksums.sha256 written"

    # ── retention ───────────────────────────────────────────────────────────
    local -a old
    local v
    mapfile -t old < <(bundle_dirs_oldest_first "$BUILD_DIR/$build_name")
    if (( ${#old[@]} > keep )); then
        for v in "${old[@]:0:$(( ${#old[@]} - keep ))}"; do
            rm -rf -- "${BUILD_DIR:?}/$build_name/$v" && info "pruned old bundle $v"
        done
    fi

    banner "BUNDLE READY"
    field "version" "$version"
    field "path"    "${bundle#"$ROOT_DIR"/}"
    field "size"    "$(du -sh "$bundle" | cut -f1)"
}

# The cheap mistakes that build fine and ship broken.
export_assert_build_inputs() {
    [[ -f "$ROOT_DIR/Dockerfile" ]] || { err "no Dockerfile at the repository root"; exit 1; }
    grep -q '^\[profile\.release\]' "$ROOT_DIR/Cargo.toml" \
        || { err "Cargo.toml has no [profile.release] section"; exit 1; }
    # Migrations are embedded with include_str!, so the build context must
    # carry them, and every .sql file must be listed or the schema ships short.
    if [[ -d "$ROOT_DIR/migrations" ]]; then
        grep -q 'COPY migrations' "$ROOT_DIR/Dockerfile" \
            || { err "Dockerfile does not COPY migrations — include_str! cannot resolve"; exit 1; }
        local sql embedded
        sql="$(find "$ROOT_DIR/migrations" -maxdepth 1 -name '*.sql' -type f | wc -l)"
        embedded="$(grep -rho 'include_str!("[./]*migrations/[^"]*\.sql")' "$ROOT_DIR/src" | sort -u | wc -l)"
        [[ "$sql" -eq "$embedded" ]] \
            || { err "migrations/ holds $sql .sql file(s) but src embeds $embedded — the schema would ship incomplete"; exit 1; }
        ok "$sql migration(s) embedded"
    fi
}

# Prove the image starts and resolves its runtime paths before it is archived.
# Without credentials the app must report a configuration fault and STAY UP
# serving /health: rolling back cannot fix a missing secret, so a config fault
# must never look like a crash to the deploy gate.
export_accept_image() {
    local stack="$1" tag="$2" name out state user
    section "RUNTIME ACCEPTANCE"
    step "image starts without configuration, reports it, and stays up"
    name="cbl-accept-$$"
    docker rm -f "$name" >/dev/null 2>&1 || true
    docker run -d --name "$name" "$tag" >/dev/null 2>&1 || { err "image would not start at all"; exit 1; }
    sleep 6
    out="$(docker logs "$name" 2>&1 || true)"
    state="$(docker inspect -f '{{.State.Status}}' "$name" 2>/dev/null || echo missing)"
    docker rm -f "$name" >/dev/null 2>&1 || true

    grep -qF "$(stack_attr "$stack" accept_log)" <<<"$out" || {
        err "image did not report missing configuration:"; head -8 <<<"$out" >&2; exit 1; }
    grep -qF "$(stack_attr "$stack" accept_ui)" <<<"$out" || {
        err "image does not carry the built UI:"; head -8 <<<"$out" >&2; exit 1; }
    [[ "$state" == running ]] || {
        err "image exited on missing configuration; it must stay up and serve /health"
        head -8 <<<"$out" >&2; exit 1; }
    ok "reports the fault, carries the UI, and stays up"

    step "runs as a non-root user"
    user="$(docker run --rm --entrypoint id "$tag" -un 2>/dev/null || echo root)"
    [[ "$user" != root ]] || { err "image runs as root"; exit 1; }
    ok "runs as $user"
}

export_stage_files() {
    local stack="$1" paths_file="$2" bundle="$3" src="$RM_DIR/stacks/$stack"
    local compose_name deploy_name rollback_name guide_name
    compose_name="$(stack_attr "$stack" compose)"
    deploy_name="$(stack_attr "$stack" deploy)"
    rollback_name="$(stack_attr "$stack" rollback)"
    guide_name="$(stack_attr "$stack" guide)"

    export_assert_env_keys "$src/.env.example"

    cp "$src/$compose_name"  "$bundle/$compose_name"
    cp "$src/$deploy_name"   "$bundle/$deploy_name"
    cp "$src/$rollback_name" "$bundle/$rollback_name"
    cp "$src/.env.example"   "$bundle/.env.example"
    [[ -f "$src/$guide_name" ]] && cp "$src/$guide_name" "$bundle/$guide_name"
    # The shared runtime travels with every bundle, so the stack directory on
    # the VPS is self-sufficient over nothing but SSH.
    cp "$RM_DIR/stacks/_shared/_cbl_lib.sh" "$RM_DIR/stacks/_shared/_cbl_deploy.sh" \
       "$RM_DIR/stacks/_shared/_cbl_rollback.sh" "$bundle/"
    chmod +x "$bundle/$deploy_name" "$bundle/$rollback_name"

    # The compose file bind-mounts ./sidecar, and a bind mount of a missing path
    # silently becomes an empty DIRECTORY: the container would then die on
    # `cp /srv/sidecar/*.mjs`. Gate on the scripts, and stage .mjs only, so no
    # stray file reaches a container that holds a PIN and a live TOTP.
    if grep -q '\./sidecar:' "$bundle/$compose_name"; then
        local script image_pw npm_pw
        for script in server.mjs login.mjs prompt.mjs reap.mjs; do
            [[ -f "$ROOT_DIR/sidecar/$script" ]] \
                || { err "compose mounts ./sidecar but sidecar/$script is missing"; exit 1; }
        done
        mkdir -p "$bundle/sidecar"
        find "$ROOT_DIR/sidecar" -maxdepth 1 -type f -name '*.mjs' -exec cp -- {} "$bundle/sidecar/" \;
        # The image tag and the npm pin must agree, or the package expects
        # browser revisions the image does not carry.
        image_pw="$(sed -n 's|.*mcr\.microsoft\.com/playwright:v\([0-9.]*\)-.*|\1|p' "$bundle/$compose_name" | head -1)"
        npm_pw="$(sed -n 's|.*playwright@\([0-9.]*\).*|\1|p' "$bundle/$compose_name" | head -1)"
        [[ -n "$image_pw" && "$image_pw" == "$npm_pw" ]] \
            || { err "playwright image tag ($image_pw) and npm pin ($npm_pw) disagree"; exit 1; }
        ok "staged $(find "$bundle/sidecar" -type f | wc -l) sidecar script(s), playwright $image_pw"
    fi

    # paths.json is copied byte for byte — never generated here.
    cp "$paths_file" "$bundle/paths.json"
    ok "staged compose, scripts, .env.example and paths.json"
}

# The stack's .env.example and the app's documented variables must declare the
# same keys. A reference file that lies is worse than none, and nothing else
# would catch it. Compared on names only: values differ on purpose.
export_assert_env_keys() {
    local stack_example="$1" app_example="$ROOT_DIR/config/env.example" drift
    [[ -f "$app_example" ]] || { warn "no config/env.example to compare against"; return 0; }
    env_keys() { grep -oE '^[A-Za-z_][A-Za-z0-9_]*=' "$1" | tr -d '=' | sort -u; }
    if ! drift="$(diff <(env_keys "$app_example") <(env_keys "$stack_example"))"; then
        err "the stack .env.example declares different variables than config/env.example"
        sed 's/^</  only in config:  /; s/^>/  only in stack: /' <<<"$drift" >&2
        exit 1
    fi
    ok ".env.example key sets agree ($(env_keys "$app_example" | wc -l) variables)"
}
