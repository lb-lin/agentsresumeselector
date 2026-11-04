#!/bin/sh
# Installs `agents` (and fzf when it is missing or too old) into ~/.local/bin. No root needed.
#
#   curl -fsSL https://raw.githubusercontent.com/lb-lin/agentsresumeselector/main/install.sh | sh
#
# AGENTS_BIN_DIR       where to install (default ~/.local/bin)
# AGENTS_REF           the release to install, a tag such as v0.9.0 (default the newest)
# AGENTS_DOWNLOAD_URL  take the program from here instead of the GitHub release
# AGENTS_SKIP_FZF      set to 1 to leave fzf alone
set -eu

REPO="lb-lin/agentsresumeselector"
REF="${AGENTS_REF:-}"
BIN="${AGENTS_BIN_DIR:-$HOME/.local/bin}"
FZF_MIN="0.65"  # --footer, which the list switcher's layout uses

say() { printf '%s\n' "$*" >&2; }
die() { say "agents: $*"; exit 1; }

if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL "$1" -o "$2"; }
  final_url() { curl -fsSLI -o /dev/null -w '%{url_effective}' "$1"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -qO "$2" "$1"; }
  # the last redirect's target; BusyBox wget has no --max-redirect, GNU wget adds " [following]"
  final_url() { wget -S --spider "$1" 2>&1 | sed -n 's/^ *[Ll]ocation: *\([^ ]*\).*$/\1/p' | tail -n 1 | tr -d '\r'; }
else
  die "curl or wget is needed"
fi

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | cut -d' ' -f1
  elif command -v sha256 >/dev/null 2>&1; then sha256 -q "$1"
  else echo ""; fi
}

# is version $1 at least $2 (major.minor)?
at_least() {
  awk -v a="$1" -v b="$2" 'BEGIN { split(a, x, "."); split(b, y, "."); exit !((x[1] > y[1]) || (x[1] == y[1] && x[2] >= y[2])) }'
}

mkdir -p "$BIN"
# downloads land next to where they go, so moving them in is a rename, never a half-copied file
tmp="$(mktemp -d "$BIN/.agents.XXXXXX")" || die "cannot write to $BIN"
trap 'rm -rf "$tmp"' EXIT INT TERM

# --- agents ---
# a static program for this machine, or, where none is built, compiled here
case "$(uname -s)/$(uname -m)" in
  Linux/x86_64|Linux/amd64) target=x86_64-unknown-linux-musl ;;
  Linux/aarch64|Linux/arm64) target=aarch64-unknown-linux-musl ;;
  Linux/armv7*|Linux/armv8l) target=armv7-unknown-linux-musleabihf ;;
  Linux/armv6*) target=arm-unknown-linux-musleabihf ;;
  Linux/ppc64le) target=powerpc64le-unknown-linux-musl ;;
  Darwin/x86_64) target=x86_64-apple-darwin ;;
  Darwin/arm64|Darwin/aarch64) target=aarch64-apple-darwin ;;
  *) target="" ;;
esac

if [ -n "$target" ]; then
  if [ -n "${AGENTS_DOWNLOAD_URL:-}" ]; then base="$AGENTS_DOWNLOAD_URL"
  elif [ -n "$REF" ]; then base="https://github.com/$REPO/releases/download/$REF"
  else base="https://github.com/$REPO/releases/latest/download"
  fi
  asset="agents-$target"
  say "==> agents ${REF:-newest} ($target)"
  fetch "$base/$asset" "$tmp/$asset" || die "download failed: $base/$asset"
  fetch "$base/SHA256SUMS" "$tmp/agents-sums" || die "download failed: $base/SHA256SUMS"
  expected="$(grep "  $asset\$" "$tmp/agents-sums" | cut -d' ' -f1)"
  actual="$(sha256 "$tmp/$asset")"
  [ -n "$expected" ] || die "the checksum list has no $asset"
  [ -n "$actual" ] || die "sha256sum or shasum is needed to check the download"
  [ "$expected" = "$actual" ] || die "checksum mismatch for $asset"
  chmod 755 "$tmp/$asset"
  "$tmp/$asset" --version >/dev/null 2>&1 || die "$asset does not run on this machine"
  mv "$tmp/$asset" "$BIN/agents"
  say "    $BIN/agents (checksum ok)"
else
  command -v cargo >/dev/null 2>&1 || die "no ready-made agents for $(uname -s)/$(uname -m): install Rust (https://rustup.rs) and run this again to compile it here"
  say "==> agents ${REF:-newest}: no ready-made program for $(uname -s)/$(uname -m), compiling with cargo"
  # shellcheck disable=SC2086  # the tag words split on purpose
  cargo install --quiet --locked --root "$tmp/build" --git "https://github.com/$REPO" ${REF:+--tag "$REF"} agents || die "cargo install failed"
  mv "$tmp/build/bin/agents" "$BIN/agents"
  say "    $BIN/agents"
fi

# --- fzf ---
# agents runs the fzf beside itself when there is one, else the one on PATH: check that one
need_fzf=1
probe=fzf
if [ -x "$BIN/fzf" ]; then probe="$BIN/fzf"; fi
if [ "${AGENTS_SKIP_FZF:-}" = "1" ]; then
  need_fzf=0
elif command -v "$probe" >/dev/null 2>&1; then
  have="$("$probe" --version 2>/dev/null | cut -d' ' -f1)"
  if [ -n "$have" ] && at_least "$have" "$FZF_MIN"; then
    need_fzf=0
    say "==> fzf $have already here"
  else
    say "==> fzf ${have:-?} is older than $FZF_MIN; installing a newer one in $BIN"
  fi
fi

if [ "$need_fzf" = "1" ]; then
  case "$(uname -s)" in
    Linux) os=linux ;;
    Darwin) os=darwin ;;
    FreeBSD) os=freebsd ;;
    OpenBSD) os=openbsd ;;
    *) die "this installer does not fetch fzf for $(uname -s): install fzf $FZF_MIN or newer yourself" ;;
  esac
  case "$(uname -m)" in
    x86_64|amd64) arch=amd64 ;;
    aarch64|arm64) arch=arm64 ;;
    armv7*|armv8l) arch=armv7 ;;
    armv6*) arch=armv6 ;;
    armv5*) arch=armv5 ;;
    riscv64|ppc64le|s390x) arch="$(uname -m)" ;;
    loongarch64) arch=loong64 ;;
    *) die "this installer does not fetch fzf for $(uname -m): install fzf $FZF_MIN or newer yourself" ;;
  esac
  tag="$(final_url https://github.com/junegunn/fzf/releases/latest | sed 's#.*/tag/##')"
  case "$tag" in v[0-9]*) ;; *) die "could not find fzf's latest release" ;; esac
  version="${tag#v}"
  asset="fzf-$version-${os}_$arch.tar.gz"
  base="https://github.com/junegunn/fzf/releases/download/$tag"
  say "==> fzf $version ($os/$arch)"
  fetch "$base/$asset" "$tmp/$asset" || die "fzf download failed"
  fetch "$base/fzf_${version}_checksums.txt" "$tmp/sums" || die "fzf checksums download failed"
  expected="$(grep "  $asset\$" "$tmp/sums" | cut -d' ' -f1)"
  actual="$(sha256 "$tmp/$asset")"
  [ -n "$expected" ] || die "fzf's checksum list has no $asset"
  [ -n "$actual" ] || die "sha256sum or shasum is needed to check fzf"
  [ "$expected" = "$actual" ] || die "fzf checksum mismatch"
  # -o: the files belong to whoever installs, not to the uid inside the archive (matters as root)
  tar -xzof "$tmp/$asset" -C "$tmp" fzf
  mv "$tmp/fzf" "$BIN/fzf"
  chmod 755 "$BIN/fzf"
  say "    $BIN/fzf (checksum ok)"
fi

case ":$PATH:" in
  *":$BIN:"*) ;;
  *)
    say ""
    say "$BIN is not on PATH. Add this to your shell's rc file, then open a new shell:"
    say "  export PATH=\"$BIN:\$PATH\""
    ;;
esac

say ""
say "done: run \`agents\` (\`agents --help\` for more)"
