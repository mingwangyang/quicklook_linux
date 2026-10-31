#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
if (( EUID == 0 )); then
    printf 'Install Rust as your desktop user, without sudo.\n' >&2
    exit 2
fi
rust_version=$(sed -n 's/^channel = "\([^"]*\)"$/\1/p' "$project_root/rust-toolchain.toml")
[[ "$rust_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { printf 'Expected a pinned Rust version.\n' >&2; exit 2; }
cargo_dir=${CARGO_HOME:-"$HOME/.cargo"}
log_dir="$project_root/.local/dev-environment"
mkdir -p "$log_dir"
stamp=$(TZ=Asia/Shanghai date +%Y%m%dT%H%M%S%z)
log_file="$log_dir/rust-install-$stamp.log"
exec > >(tee -a "$log_file") 2>&1
trap 'status=$?; printf "\nFinished: %s; exit=%s\nLog: %s\n" "$(TZ=Asia/Shanghai date -Iseconds)" "$status" "$log_file"' EXIT
printf 'Started: %s\nRust: %s\n' "$(TZ=Asia/Shanghai date -Iseconds)" "$rust_version"

if [[ -x "$cargo_dir/bin/rustup" ]]; then
    rustup_cmd="$cargo_dir/bin/rustup"
elif command -v rustup >/dev/null; then
    rustup_cmd=$(command -v rustup)
else
    installer="$log_dir/rustup-init.sh"
    curl --proto '=https' --tlsv1.2 -fLsS --connect-timeout 15 --max-time 120 \
        https://sh.rustup.rs -o "$installer.download"
    # Bootstrap script downloaded from the official endpoint on 2026-10-25.
    expected_sha=9502b514362a21096210d498f143b8cdacda365d3dfcf45e8f5541e089ccafc0
    printf '%s  %s\n' "$expected_sha" "$installer.download" | sha256sum --check
    mv -- "$installer.download" "$installer"
    printf 'Bootstrap URL: https://sh.rustup.rs\n'
    sha256sum "$installer"
    sh "$installer" -y --no-modify-path --profile minimal --default-toolchain "$rust_version" \
        --component rustfmt --component clippy
    rustup_cmd="$cargo_dir/bin/rustup"
fi

"$rustup_cmd" toolchain install "$rust_version" --profile minimal --component rustfmt --component clippy
"$rustup_cmd" --version
"$rustup_cmd" run "$rust_version" rustc --version --verbose
"$rustup_cmd" run "$rust_version" cargo --version
"$rustup_cmd" run "$rust_version" rustfmt --version
"$rustup_cmd" run "$rust_version" cargo-clippy --version
printf '\nFor this shell: source "%s/env"\n' "$cargo_dir"
