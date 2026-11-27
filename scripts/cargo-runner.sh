#!/usr/bin/env bash
# Cargo runner (see .cargo/config.toml) that picks what `cargo run` does with the desktop app:
#
#   cargo run                      runs the app
#   cargo run desktop [OPTIONS]    runs it under `dx serve --hot-patch`, which patches each save
#                                  into the open window (OPTIONS go to dx)
#   cargo run android [OPTIONS]    builds the Android client and opens it (scripts/cargo-android)
#
# Every other binary, and the app with any other arguments, runs as it is.
set -euo pipefail

bin=${1:?the runner needs a binary}
case "$(basename "$bin" .exe) ${2:-}" in
    "patchwork desktop") mode=desktop ;;
    "patchwork android") mode=android ;;
    *) exec "$@" ;;
esac
shift 2
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

if [[ $mode == android ]]; then
    # `cargo run --release android` builds the release APK.
    [[ $bin == */release/* ]] && set -- --release "$@"
    exec "$root/scripts/cargo-android" "$@"
fi

if ! command -v dx >/dev/null; then
    echo "hot reload needs the dx CLI: cargo binstall dioxus-cli@0.7.10" >&2
    exit 1
fi
# GPUI's `hot-reload` feature connects the app to dx and renders every view through a function
# dx can patch. After a change a patch can't carry, such as a struct's fields, press `r` in dx's
# terminal to rebuild and restart the app.
cd "$root"
exec dx serve --desktop --hot-patch --package patchwork --features hot-reload "$@"
