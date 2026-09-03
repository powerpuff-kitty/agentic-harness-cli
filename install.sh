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

if [ -z "$BINARY" ]; then
  if [ -x "$ROOT/target/release/ah" ]; then
    BINARY="$ROOT/target/release/ah"
  elif command -v cargo >/dev/null 2>&1; then
    if [ ! -d "$ROOT/upstream/agentic-harness" ]; then "$ROOT/scripts/sync-upstream.sh"; fi
    cargo build --release --manifest-path "$ROOT/Cargo.toml"
    BINARY="$ROOT/target/release/ah"
  else
    echo "No compiled ah binary found. Pass --binary or install Rust to build from source." >&2
    exit 127
  fi
fi

mkdir -p "$PREFIX/bin"
cp "$BINARY" "$PREFIX/bin/$COMMAND"
chmod +x "$PREFIX/bin/$COMMAND"
echo "Installed $COMMAND to $PREFIX/bin/$COMMAND"
