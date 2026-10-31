#!/usr/bin/env bash
# Run as the normal desktop user; only apt commands use sudo.
set -euo pipefail

project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
mode=${1:---dry-run}
case "$mode" in
    --dry-run|--install) ;;
    *) printf 'Usage: bash scripts/install-system-deps.sh [--dry-run|--install]\n' >&2; exit 2 ;;
esac
if (( EUID == 0 )); then
    printf 'Run this script as your desktop user, without sudo. It invokes sudo for apt.\n' >&2
    exit 2
fi
if ! command -v apt-get >/dev/null; then
    printf 'This package list supports Linux Mint / Ubuntu with apt-get.\n' >&2
    exit 2
fi

log_dir="$project_root/.local/dev-environment"
mkdir -p "$log_dir"
stamp=$(TZ=Asia/Shanghai date +%Y%m%dT%H%M%S%z)
log_file="$log_dir/system-deps-$stamp.log"
exec > >(tee -a "$log_file") 2>&1
trap 'status=$?; printf "\nFinished: %s; exit=%s\nLog: %s\n" "$(TZ=Asia/Shanghai date -Iseconds)" "$status" "$log_file"' EXIT
printf 'Started: %s\nMode: %s\n' "$(TZ=Asia/Shanghai date -Iseconds)" "$mode"

packages=()
while IFS= read -r package || [[ -n "$package" ]]; do
    [[ -z "$package" || "$package" == \#* ]] && continue
    [[ "$package" =~ ^[a-z0-9][a-z0-9+.-]*$ ]] || { printf 'Invalid package: %s\n' "$package" >&2; exit 2; }
    packages+=("$package")
done < "$project_root/scripts/dev-packages.txt"
printf '\nRequested packages:\n'
printf '%s\n' "${packages[@]}"

if [[ "$mode" == --dry-run ]]; then
    apt-get --simulate --no-remove install --no-install-recommends "${packages[@]}"
else
    sudo apt-get update
    sudo apt-get --no-remove install --no-install-recommends "${packages[@]}"
    printf '\nInstalled package versions:\n'
    dpkg-query -W -f='${binary:Package}\t${Version}\t${db:Status-Abbrev}\n' "${packages[@]}"
fi
