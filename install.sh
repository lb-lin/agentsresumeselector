#!/bin/sh
# Installs `agents` (and fzf when it is missing or too old) into ~/.local/bin. No root needed.
#
#   curl -fsSL https://raw.githubusercontent.com/lb-lin/agentsresumeselector/main/install.sh | sh
#
# AGENTS_BIN_DIR   where to install (default ~/.local/bin)
# AGENTS_REF       branch or tag of this repository to install (default main)
# AGENTS_SKIP_FZF  set to 1 to leave fzf alone
set -eu

REPO="lb-lin/agentsresumeselector"
REF="${AGENTS_REF:-main}"
BIN="${AGENTS_BIN_DIR:-$HOME/.local/bin}"
FZF_MIN="0.46"

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

command -v python3 >/dev/null 2>&1 || die "python3 is needed (3.8 or newer)"
# -I: never import modules from the folder the installer runs in
python3 -I -c 'import sys; sys.exit(sys.version_info < (3, 8))' || die "python3 3.8 or newer is needed"

mkdir -p "$BIN"
# downloads land next to where they go, so moving them in is a rename, never a half-copied file
tmp="$(mktemp -d "$BIN/.agents.XXXXXX")" || die "cannot write to $BIN"
trap 'rm -rf "$tmp"' EXIT INT TERM

# --- agents ---
say "==> agents ($REF)"
fetch "https://raw.githubusercontent.com/$REPO/$REF/agents" "$tmp/agents" || die "download failed"
head -n 1 "$tmp/agents" | grep -q '^#!/usr/bin/env python3' || die "what was downloaded is not the agents script"
python3 -I -c 'import ast, sys; ast.parse(open(sys.argv[1], encoding="utf-8").read())' "$tmp/agents" || die "the downloaded script does not parse"
mv "$tmp/agents" "$BIN/agents"
chmod 755 "$BIN/agents"
say "    $BIN/agents"

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
