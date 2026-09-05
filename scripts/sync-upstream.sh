#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
LOCK="$ROOT/upstream.lock.json"

CANONICAL_COMMIT=$(sed -n '/"canonical"/,/}/s/.*"commit": "\([^"]*\)".*/\1/p' "$LOCK")
AGENTS_COMMIT=$(sed -n '/"agents"/,/}/s/.*"commit": "\([^"]*\)".*/\1/p' "$LOCK")

rm -rf "$ROOT/upstream/agentic-harness" "$ROOT/upstream/agentic-harness-agents"
mkdir -p "$ROOT/upstream"

git clone --quiet https://github.com/powerpuff-kitty/agentic-harness.git "$ROOT/upstream/agentic-harness"
git -C "$ROOT/upstream/agentic-harness" checkout --quiet "$CANONICAL_COMMIT"

git clone --quiet https://github.com/powerpuff-kitty/agentic-harness-agents.git "$ROOT/upstream/agentic-harness-agents"
git -C "$ROOT/upstream/agentic-harness-agents" checkout --quiet "$AGENTS_COMMIT"

"$ROOT/scripts/prepare-upstream.sh"
echo "Synced canonical=$CANONICAL_COMMIT agents=$AGENTS_COMMIT"
