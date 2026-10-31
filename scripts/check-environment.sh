#!/usr/bin/env bash
# Read-only checks, with a local log. --gpu also connects to the current Wayland session.
set -euo pipefail
project_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
mode=${1:---check}
case "$mode" in
    --check|--gpu) ;;
    *) printf 'Usage: bash scripts/check-environment.sh [--check|--gpu]\n' >&2; exit 2 ;;
esac
cargo_dir=${CARGO_HOME:-"$HOME/.cargo"}
if [[ -f "$cargo_dir/env" ]]; then
    source "$cargo_dir/env"
fi
cd "$project_root"
log_dir="$project_root/.local/dev-environment"
mkdir -p "$log_dir"
stamp=$(TZ=Asia/Shanghai date +%Y%m%dT%H%M%S%z)
log_file="$log_dir/check-$stamp.log"
exec > >(tee -a "$log_file") 2>&1
printf 'Checked: %s\nLog: %s\n' "$(TZ=Asia/Shanghai date -Iseconds)" "$log_file"
printf 'Session: %s; desktop: %s; Wayland: %s\n' "${XDG_SESSION_TYPE:-unset}" "${XDG_CURRENT_DESKTOP:-unset}" "${WAYLAND_DISPLAY:-unset}"

failures=0
for tool in cc c++ pkg-config cmake meson ninja rustup rustc cargo rustfmt cargo-clippy ffprobe gs fzf wl-copy wayland-info vainfo; do
    if command -v "$tool" >/dev/null; then
        printf 'OK tool %-20s %s\n' "$tool" "$(command -v "$tool")"
    else
        printf 'MISSING tool %s\n' "$tool"
        failures=$((failures + 1))
    fi
done
rust_version=$(sed -n 's/^channel = "\([^"]*\)"$/\1/p' rust-toolchain.toml)
if command -v rustup >/dev/null; then
    if rustup run "$rust_version" rustc --version && rustup run "$rust_version" cargo --version \
        && rustup run "$rust_version" rustfmt --version && rustup run "$rust_version" cargo-clippy --version; then
        printf 'OK pinned Rust toolchain %s\n' "$rust_version"
    else
        failures=$((failures + 1))
    fi
fi

modules=(gtk4 glib-2.0 gio-2.0 epoxy wayland-client xkbcommon librsvg-2.0 gtksourceview-5 poppler-glib mpv libarchive webkitgtk-6.0 libheif libraw libjxl libtiff-4 vips fontconfig freetype2 lcms2 libva)
for module in "${modules[@]}"; do
    if version=$(pkg-config --modversion "$module" 2>/dev/null); then
        printf 'OK library %-20s %s\n' "$module" "$version"
    else
        printf 'MISSING library %s\n' "$module"
        failures=$((failures + 1))
    fi
done

missing_packages=()
native_arch=$(dpkg --print-architecture)
while IFS= read -r package || [[ -n "$package" ]]; do
    [[ -z "$package" || "$package" == \#* ]] && continue
    # A multiarch package can have both amd64 and i386 rows. Require the native
    # architecture (or Architecture: all), rather than concatenating statuses.
    if ! dpkg-query -W -f='${Architecture}\t${db:Status-Status}\n' "$package" 2>/dev/null \
        | awk -F '\t' -v arch="$native_arch" '$2 == "installed" && ($1 == arch || $1 == "all") { found = 1 } END { exit !found }'; then
        missing_packages+=("$package")
    fi
done < scripts/dev-packages.txt
if (( ${#missing_packages[@]} )); then
    printf '\nMissing apt packages:\n'
    printf '%s\n' "${missing_packages[@]}"
    failures=$((failures + 1))
fi

if [[ "$mode" == --gpu ]]; then
    printf '\nWayland protocols:\n'
    if ! wayland-info | awk '/interface:.*(xdg_wm_base|layer_shell|fractional_scale|viewporter|presentation|activation)/'; then
        failures=$((failures + 1))
    fi
    printf '\nVA-API driver and supported profiles:\n'
    if ! vainfo --display wayland; then
        failures=$((failures + 1))
    fi
fi
printf '\nEnvironment check: %s missing or failed check(s).\n' "$failures"
(( failures == 0 ))
