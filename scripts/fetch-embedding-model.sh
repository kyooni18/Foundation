#!/bin/sh
# Download nvidia/Nemotron-3-Embed-1B-BF16 (OpenMDW-1.1) at a pinned revision and verify it.
# Files are data only (safetensors weights, JSON configs); no repository code is executed.
# Usage: scripts/fetch-embedding-model.sh [DEST_DIR]
set -eu
REV=c0c9fea93ea424587517f2c59e20db9f1d6bf615
DEST=${1:-"$HOME/Library/Application Support/PrimeRadiant/models/nemotron-3-embed-1b"}
BASE="https://huggingface.co/nvidia/Nemotron-3-Embed-1B-BF16/resolve/$REV"
mkdir -p "$DEST"
fetch() { # remote-path sha256
  out="$DEST/$1"
  if [ ! -f "$out" ]; then
    curl -fsSL --retry 4 -o "$out.part" "$BASE/$1"
    mv "$out.part" "$out"
  fi
  got=$(shasum -a 256 "$out" | cut -d' ' -f1)
  [ "$got" = "$2" ] || { rm -f "$out"; echo "sha256 mismatch for $1" >&2; exit 1; }
}
fetch model.safetensors f959c3b04e66b42de280bfb97c140cb7e0bfe25e3ecb0b4464c68a8436b2d04f
fetch tokenizer.json 797410dfb649a5b9ba92bc4fef7dbf4022d00e73de6867c4ac199a8846439421
# Small configs are pinned by revision; their digests are checked by the runtime manifest.
for f in config.json LICENSE NOTICE; do
  [ -f "$DEST/$f" ] || curl -fsSL --retry 4 -o "$DEST/$f" "$BASE/$f"
done
printf '%s\n' "$REV" > "$DEST/REVISION"
echo "verified nvidia/Nemotron-3-Embed-1B-BF16@$REV in $DEST"
