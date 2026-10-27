#!/usr/bin/env bash
# Uploads release files to the Bunny storage zone behind updates.patchworkeditor.com, into the
# tag's folder, and prints a JSON array describing them: each file's name, SHA-256 digest, and the
# text of its .sig file when it has one. The release job builds latest.json from these arrays.
#
# Usage: BUNNY_STORAGE_PASSWORD=... scripts/publish-files.sh v0.2.0 FILE...
set -euo pipefail

storage=https://ny.storage.bunnycdn.com/patchwork-releases
tag="$1"
shift
[[ -n "${BUNNY_STORAGE_PASSWORD:-}" ]] || { echo "BUNNY_STORAGE_PASSWORD is not set." >&2; exit 1; }

assets='[]'
for file in "$@"; do
  name=$(basename "$file")
  curl -fsS --retry 3 -T "$file" -H "AccessKey: $BUNNY_STORAGE_PASSWORD" \
    -H "Content-Type: application/octet-stream" "$storage/$tag/$name" > /dev/null
  sig=null
  [[ -f "$file.sig" ]] && sig=$(jq -Rs . < "$file.sig")
  # macOS has shasum but not always sha256sum.
  digest=$( (sha256sum "$file" 2> /dev/null || shasum -a 256 "$file") | cut -d' ' -f1)
  assets=$(jq -c --arg name "$name" --arg digest "sha256:$digest" --argjson sig "$sig" \
    '. + [{name: $name, digest: $digest, sig: $sig}]' <<< "$assets")
done
echo "$assets"
