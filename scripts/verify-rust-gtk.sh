#!/usr/bin/env bash
# Compile a real Rust/GTK4 program and initialize native Wayland without opening a window.
set -euo pipefail
project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
mode=${1:---basic}
features=()
case "$mode" in
    --basic) ;;
    --full) features=(--features full) ;;
    *) printf 'Usage: bash scripts/verify-rust-gtk.sh [--basic|--full]\n' >&2; exit 2 ;;
esac
cargo_dir=${CARGO_HOME:-"$HOME/.cargo"}
if [[ -f "$cargo_dir/env" ]]; then
    source "$cargo_dir/env"
fi
cd "$project_root"
log_dir="$project_root/.local/dev-environment"
mkdir -p "$log_dir"
stamp=$(TZ=Asia/Shanghai date +%Y%m%dT%H%M%S%z)
log_file="$log_dir/rust-gtk-$stamp.log"
exec > >(tee -a "$log_file") 2>&1
trap 'status=$?; printf "\nFinished: %s; exit=%s\nLog: %s\n" "$(TZ=Asia/Shanghai date -Iseconds)" "$status" "$log_file"' EXIT
printf 'Started: %s\nMode: %s\n' "$(TZ=Asia/Shanghai date -Iseconds)" "$mode"
GDK_BACKEND=wayland CARGO_TARGET_DIR="$project_root/target/environment-check" \
    cargo run --locked --manifest-path tools/environment-check/Cargo.toml "${features[@]}"
