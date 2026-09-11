#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
COMMAND=${AGENTIC_HARNESS_COMMAND:-ah}
PREFIX=${PREFIX:-/usr/local}
BINARY=

while [ "$#" -gt 0 ]; do
  case "$1" in
    --command) shift; COMMAND=${1:?--command requires a value} ;;
    --prefix) shift; PREFIX=${1:?--prefix requires a value} ;;
    --binary) shift; BINARY=${1:?--binary requires a value} ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

case "$COMMAND" in
  ""|.|..|*/*|*\\*) echo "--command must be a filename" >&2; exit 2 ;;
esac

if [ -z "$BINARY" ]; then
  if [ -x "$ROOT/target/release/ah" ]; then
    BINARY="$ROOT/target/release/ah"
  elif command -v cargo >/dev/null 2>&1; then
    if [ ! -d "$ROOT/upstream/agentic-harness" ]; then "$ROOT/scripts/sync-upstream.sh"; fi
    cargo build --locked --release --bin ah --manifest-path "$ROOT/Cargo.toml"
    BINARY="$ROOT/target/release/ah"
  else
    echo "No compiled ah binary found. Pass --binary or install Rust to build from source." >&2
    exit 127
  fi
fi

mkdir -p "$PREFIX/bin"
INSTALL_STAGE=$(mktemp "$PREFIX/bin/.ah-install.XXXXXX")
trap 'rm -f "$INSTALL_STAGE"' EXIT HUP INT TERM
cp "$BINARY" "$INSTALL_STAGE"
chmod +x "$INSTALL_STAGE"
"$INSTALL_STAGE" --version >/dev/null
mv -f "$INSTALL_STAGE" "$PREFIX/bin/$COMMAND"
echo "Installed $COMMAND to $PREFIX/bin/$COMMAND"
