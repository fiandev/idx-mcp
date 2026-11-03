#!/bin/sh
# Installs the idx binary from GitHub releases with SHA-256 verification.
#
#   curl -fsSL https://raw.githubusercontent.com/fzn0x/idx/main/install.sh | sh
#
# Environment:
#   IDX_VERSION      tag to install (default: latest release), e.g. v0.1.0
#   IDX_INSTALL_DIR  directory for the binary (default: ~/.local/bin)
#   IDX_REPO         GitHub repo (default: fzn0x/idx)
set -eu

REPO="${IDX_REPO:-fzn0x/idx}"
INSTALL_DIR="${IDX_INSTALL_DIR:-$HOME/.local/bin}"
BIN=idx

say() { printf '%s\n' "$*" >&2; }
die() { say "install.sh: $*"; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || die "missing dependency: $1"; }
# A server that accepts the connection and then sends nothing fails after a
# minute instead of hanging the install.
dl() { curl -fsSL --connect-timeout 20 --speed-limit 1 --speed-time 60 "$@"; }

need curl
need tar

os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
  Linux)  os_part=unknown-linux-musl ;;
  Darwin) os_part=apple-darwin ;;
  MINGW*|MSYS*|CYGWIN*) die "on Windows, download the .zip from https://github.com/$REPO/releases or use: cargo install --git https://github.com/$REPO" ;;
  *) die "unsupported OS: $os" ;;
esac
case "$arch" in
  x86_64|amd64)  arch_part=x86_64 ;;
  arm64|aarch64) arch_part=aarch64 ;;
  *) die "unsupported architecture: $arch" ;;
esac
target="${arch_part}-${os_part}"

if [ -n "${IDX_VERSION:-}" ]; then
  tag="$IDX_VERSION"
else
  tag="$(dl "https://api.github.com/repos/$REPO/releases/latest" \
    | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n1)"
  [ -n "$tag" ] || die "could not determine the latest release of $REPO"
fi
version="${tag#v}"
asset="${BIN}-${version}-${target}.tar.gz"
base="https://github.com/$REPO/releases/download/$tag"

tmp="$(mktemp -d 2>/dev/null || mktemp -d -t idx)"
trap 'rm -rf "$tmp"' EXIT

say "Downloading $asset ($tag)"
dl -o "$tmp/$asset" "$base/$asset" || die "no release asset $asset at $base (release may not include $target)"
dl -o "$tmp/SHA256SUMS" "$base/SHA256SUMS" || die "no SHA256SUMS in release $tag"

expected="$(grep " $asset\$" "$tmp/SHA256SUMS" | awk '{print $1}')"
[ -n "$expected" ] || die "$asset is not listed in SHA256SUMS"
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$tmp/$asset" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  actual="$(shasum -a 256 "$tmp/$asset" | awk '{print $1}')"
else
  die "need sha256sum or shasum to verify the download"
fi
[ "$expected" = "$actual" ] || die "checksum mismatch for $asset (expected $expected, got $actual)"
say "Checksum verified"

tar -xzf "$tmp/$asset" -C "$tmp"
mkdir -p "$INSTALL_DIR"
install -m 755 "$tmp/${BIN}-${version}-${target}/$BIN" "$INSTALL_DIR/$BIN"
say "Installed $INSTALL_DIR/$BIN ($("$INSTALL_DIR/$BIN" --version 2>/dev/null || echo "$tag"))"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) say "Add it to your PATH, e.g.:  export PATH=\"$INSTALL_DIR:\$PATH\"" ;;
esac
say "Try:  idx stocks quote BBCA      Agents:  claude mcp add idx -- idx mcp"
