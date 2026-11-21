#!/bin/bash
# Packs the published test data (docs/development.md "Set up") from the maintainer's stores into OUT/<tag>/:
#   recordings.tar.zst  the files in RBR_RECORDINGS that data/test-data.recordings lists (RSF sessions stripped)
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
# An RSF session file goes without the game's and the car packs' files (harness/session.py --strip).
grep -v '^#' data/test-data.recordings | while read -r f; do
    case $f in
    rsf/*/session.bin)
        mkdir -p "$out/$tag/recordings/$(dirname "$f")"
        python3 -I -B harness/session.py --strip "$recordings/$f" "$out/$tag/recordings/$f" >/dev/null ;;
    rsf/*/files/*) echo "$f: the game's or a car pack's file" >&2; exit 1 ;;
    rsf/*/setup.lsp)  # only the player's own saved setup
        grep -q '^setup=savedgames/' "$recordings/$(dirname "$f")/attempt.txt" ||
            { echo "$f: not a saved setup (a car pack's file)" >&2; exit 1; }
        stage recordings "$recordings/$f" "$f" ;;
    *) stage recordings "$recordings/$f" "$f" ;;
    esac
done

# bsdtar (macOS) without its metadata; GNU tar without owners
if tar --version | grep -q bsdtar; then tar_flags=(--no-xattrs --no-mac-metadata); else tar_flags=(--owner=0 --group=0 --numeric-owner); fi
for part in recordings; do
    echo "$part: $(find "$out/$tag/$part" -type f | wc -l) files"
    # Sorted, without macOS metadata, so the archive unpacks cleanly with GNU tar.
    (cd "$out/$tag/$part" && find . -type f | sed 's|^\./||' | sort | tar "${tar_flags[@]}" -cf - -T -) |
        zstd -q -T0 -19 --long=31 -o "$out/$tag/$part.tar.zst"
    rm -r "${out:?}/$tag/$part"
done
(cd "$out" && shasum -a 256 "$tag"/recordings.tar.zst) > "$out/$tag.SHA256SUMS"
cat "$out/$tag.SHA256SUMS"
