#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
CANON="$ROOT/upstream/agentic-harness"

# The canonical repository moved authoring content under catalog/. The current
# CLI still embeds the legacy materialized paths, so build a disposable
# compatibility view inside upstream/ after checkout. Nothing here is canonical.
for name in base web-app backend-api saas monorepo library-sdk; do
  src="$CANON/catalog/variants/$name"
  dst="$CANON/$name"
  if [ -d "$src/files" ]; then
    rm -rf "$dst"
    mkdir -p "$dst"
    cp -R "$src/files/." "$dst/"
    cp "$src/variant.json" "$dst/boilerplate.json"
  fi
done

mkdir -p "$CANON/modules"
rm -rf "$CANON/modules/packs" "$CANON/modules/policies" "$CANON/modules/profiles" "$CANON/presets"
cp -R "$CANON/catalog/packs" "$CANON/modules/packs"
cp -R "$CANON/catalog/policies" "$CANON/modules/policies"
cp -R "$CANON/catalog/profiles" "$CANON/modules/profiles"
cp -R "$CANON/catalog/presets" "$CANON/presets"

echo "Prepared disposable CLI compatibility view from catalog/"
