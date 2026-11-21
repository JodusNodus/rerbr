#!/bin/bash
# Packs the published test data (docs/development.md "Set up") from the maintainer's stores into OUT/<tag>/:
#   recordings.tar.zst  the files in RBR_RECORDINGS that data/test-data.recordings lists
# and OUT/<tag>.SHA256SUMS, which replaces data/test-data.SHA256SUMS. Upload the archives as the assets of a GitHub
# release named <tag> (a new tag for every new set; old checkouts keep fetching theirs).
# Usage: scripts/pack-test-data.sh TAG OUT
set -euo pipefail
cd "$(dirname "$0")/.."
tag=${1:?usage: scripts/pack-test-data.sh TAG OUT}
out=${2:?usage: scripts/pack-test-data.sh TAG OUT}
recordings=${RBR_RECORDINGS:?set RBR_RECORDINGS}
mkdir -p "$out/$tag"

# Each part is a staging folder of hard links (copies across file systems), laid out as it unpacks.
stage() {
    mkdir -p "$out/$tag/$1/$(dirname "$3")"
    ln -f "$2" "$out/$tag/$1/$3" 2>/dev/null || cp "$2" "$out/$tag/$1/$3"
}
grep -v '^#' data/test-data.recordings | while read -r f; do stage recordings "$recordings/$f" "$f"; done

for part in recordings; do
    echo "$part: $(find "$out/$tag/$part" -type f | wc -l) files"
    # Sorted, without macOS metadata, so the archive unpacks cleanly with GNU tar.
    (cd "$out/$tag/$part" && find . -type f | sed 's|^\./||' | sort |
        tar --no-xattrs --no-mac-metadata -cf - -T -) | zstd -q -T0 -19 --long=31 -o "$out/$tag/$part.tar.zst"
    rm -r "${out:?}/$tag/$part"
done
(cd "$out" && shasum -a 256 "$tag"/recordings.tar.zst) > "$out/$tag.SHA256SUMS"
cat "$out/$tag.SHA256SUMS"
