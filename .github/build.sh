#!/bin/sh
# Builds the release program for a target. Linux programs are static (musl) and linked with the
# linker that ships with Rust, so no cross compiler is needed.
set -eu
target="$1"
case "$target" in
  *-linux-musl*)
    var="CARGO_TARGET_$(printf '%s' "$target" | tr 'a-z-' 'A-Z_')_LINKER"
    export "$var=rust-lld"
    ;;
esac
cargo build --locked --release --target "$target"
