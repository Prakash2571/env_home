#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────────────────
# remote_state.sh — everything the dashboard and the diagnosis need from the
# VPS, in ONE read-only SSH round trip.
#
# It changes nothing on the VPS. It never reads .env: it reports whether the
# file exists, who owns it and its mode, and nothing else.
#
# Output is key=value lines, read back with rs <key>. A container list is a
# CONTAINERS<< … >> block. Every remote path comes from the validated contract.
# ─────────────────────────────────────────────────────────────────────────────

REMOTE_STATE=''
REMOTE_FETCHED=false

# remote_state_fetch <paths.json> — fill REMOTE_STATE once per menu render.
remote_state_fetch() {
    [[ "$REMOTE_FETCHED" == true ]] && return 0
    REMOTE_FETCHED=true
    local file="$1" a qcmd="bash -s --"
    local -a rargs=(
        "$(paths_get "$file" .vps.root)"
        "$(paths_get "$file" .vps.stack_dir)"
        "$(dirname "$(paths_get "$file" .backup.root)")"
        "$(paths_get "$file" .vps.version_name)"
        "$(paths_get "$file" .vps.env_file)"
        "$(paths_get "$file" .vps.compose_project)"
        "$(paths_get "$file" .backup.rollback_images)"
        "$(paths_get_opt "$file" .database.dump_dir)"
        "$(paths_get_opt "$file" .edge.systemd_unit)"
        "$(paths_get "$file" .health.http_url)"
    )
    # One argv element per value: the remote shell re-parses the command
    # string, so every value is quoted here.
    for a in "${rargs[@]}"; do
        printf -v a '%q' "$a"
        qcmd+=" $a"
    done

    REMOTE_STATE="$(cbl_ssh "$qcmd" <<'REMOTE' 2>/dev/null || true
set -u
root="$1"; stack_dir="$2"; backup="$3"; version_name="$4"; env_file="$5"
project="$6"; rollbacks="$7"; dumps="$8"; edge_unit="$9"; health_url="${10}"

yn() { if "$@" >/dev/null 2>&1; then echo yes; else echo no; fi; }

echo "REACHABLE=yes"
echo "USER=$(id -un)"
echo "DOCKER=$(yn docker info)"
echo "COMPOSE=$(yn docker compose version)"
for c in jq rsync flock curl sha256sum gzip; do echo "HAS_$c=$(yn command -v "$c")"; done

for pair in "ROOT=$root" "BACKUP=$backup"; do
    key="${pair%%=*}"; dir="${pair#*=}"
    if [[ ! -e "$dir" ]]; then
        echo "${key}_STATE=missing"
    elif [[ -w "$dir" ]]; then
        echo "${key}_STATE=writable"
    else
        echo "${key}_STATE=readonly"
    fi
    echo "${key}_OWNER=$(stat -c '%U:%G %a' "$dir" 2>/dev/null || echo '?')"
done
echo "STACK_PROVISIONED=$(yn test -d "$stack_dir")"
echo "SHIPPED=$(yn test -f "$stack_dir/paths.json")"

if [[ -e "$env_file" ]]; then
    echo "ENV=present"
    echo "ENV_MODE=$(stat -c '%a' "$env_file" 2>/dev/null || echo '?')"
    echo "ENV_OWNER=$(stat -c '%U:%G' "$env_file" 2>/dev/null || echo '?')"
    echo "ENV_EMPTY=$(yn test ! -s "$env_file")"
else
    echo "ENV=missing"
fi

if command -v jq >/dev/null 2>&1 && [[ -f "$stack_dir/$version_name" ]]; then
    echo "LIVE_VERSION=$(jq -r '.version // ""' "$stack_dir/$version_name" 2>/dev/null)"
    echo "LIVE_STATUS=$(jq -r '.status // ""' "$stack_dir/$version_name" 2>/dev/null)"
    echo "LIVE_PREVIOUS=$(jq -r '.previous // ""' "$stack_dir/$version_name" 2>/dev/null)"
    echo "LIVE_AT=$(jq -r '.updated_at // ""' "$stack_dir/$version_name" 2>/dev/null)"
fi

echo "ROLLBACKS=$(find "$rollbacks" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' 2>/dev/null | sort | paste -sd',' -)"
if [[ -n "$dumps" ]]; then
    echo "DUMPS=$(find "$dumps" -maxdepth 1 -type f -name '*.dump' 2>/dev/null | wc -l)"
    echo "LAST_DUMP=$(find "$dumps" -maxdepth 1 -type f -name '*.dump' -printf '%f\n' 2>/dev/null | sort | tail -n1)"
fi

if [[ -n "$edge_unit" ]]; then
    echo "EDGE=$(systemctl is-active "$edge_unit" 2>/dev/null || echo unknown)"
fi
port="${health_url#http://127.0.0.1:}"; port="${port%%/*}"
echo "PORT=$port"
echo "PORT_LISTENING=$(yn sh -c "ss -Htln 'sport = :$port' | grep -q ."))"
echo "HEALTH=$(yn curl -fsS -m 3 -o /dev/null "$health_url")"

echo "DISK_STACK=$(df -h --output=avail "$root" 2>/dev/null | tail -n1 | tr -d ' ')"
# The backup root may not exist before provisioning; measure the disk it will
# live on instead.
probe="$backup"; while [[ ! -e "$probe" && "$probe" != / ]]; do probe="$(dirname "$probe")"; done
echo "DISK_BACKUP=$(df -h --output=avail "$probe" 2>/dev/null | tail -n1 | tr -d ' ')"
echo "DISK_DOCKER=$(df -h --output=avail "$(docker info --format '{{.DockerRootDir}}' 2>/dev/null || echo /)" 2>/dev/null | tail -n1 | tr -d ' ')"
awk '/^MemTotal:/ {t=$2} /^MemAvailable:/ {a=$2} END {if (t) printf "MEM=%d MiB free of %d MiB\n", a/1024, t/1024}' /proc/meminfo

echo "CONTAINERS<<"
docker ps -a --filter "label=com.docker.compose.project=$project" \
    --format '{{.Names}}\t{{.Status}}' 2>/dev/null
echo ">>"
REMOTE
)"
    [[ -n "$REMOTE_STATE" ]] || REMOTE_STATE="REACHABLE=no"
}

remote_state_reset() { REMOTE_FETCHED=false; REMOTE_STATE=''; }

# rs <key> — one value, control characters stripped so a hostile value cannot
# drive the terminal.
rs() {
    printf '%s\n' "$REMOTE_STATE" | sed -n "s/^$1=//p" | head -n1 \
        | LC_ALL=C tr -d '\000-\010\013-\037\177'
}

rs_containers() {
    printf '%s\n' "$REMOTE_STATE" | sed -n '/^CONTAINERS<</,/^>>/p' | sed '1d;$d'
}
