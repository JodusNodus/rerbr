#!/usr/bin/env bash
# Drives rally school lessons with the autopilot (`rbr-standalone --autopilot`) and prints one line a drive: the
# lesson, the car, the verdict, how the race ended and why the drive stopped. A regression check for the school's
# wiring end to end (every lesson should end by the school, its box, its finish or a retire), not for the judging:
# the autopilot is no lesson driver, so most lessons fail on its time or line.
#
#   scripts/school-sweep.sh [-l LESSONS] [-c CARS] [-j JOBS] [-o DIR]
#
# LESSONS: comma-separated lesson indices (default all 17). CARS: comma-separated `--car` slots or RSF car pack
# folders (default 5). Uses target/release/rbr-standalone (built if missing) and dev.env's RBR_DATA and
# RBR_NGP_PHYSICS; RBR_NGP_DLL if NGP.dll is not in the install. Logs go to DIR (default a temporary folder).
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=dev-env.sh
source "$root/scripts/dev-env.sh"

lessons=$(seq -s, 0 16)
cars=5
jobs=$(($(nproc) / 2))
out=
while getopts "l:c:j:o:h" opt; do
    case $opt in
    l) lessons=$OPTARG ;;
    c) cars=$OPTARG ;;
    j) jobs=$OPTARG ;;
    o) out=$OPTARG ;;
    *)
        sed -n '2,11p' "$0"
        exit 2
        ;;
    esac
done
: "${RBR_DATA:?RBR_DATA unset (dev.env)}" "${RBR_NGP_PHYSICS:?RBR_NGP_PHYSICS unset (dev.env)}"
out=${out:-$(mktemp -d -t school-sweep.XXXXXX)}
mkdir -p "$out"

bin=$root/target/release/rbr-standalone
[[ -x $bin ]] || cargo build --release -q -p rbr-standalone --bin rbr-standalone --manifest-path "$root/Cargo.toml"

# One drive: lesson $1, car $2 (a slot number, else an RSF pack folder).
drive() {
    local lesson=$1 car=$2 flag=--car name log
    [[ $car =~ ^[0-9]+$ ]] || flag=--rsf-car
    name=$(basename "$car")
    log=$out/$lesson-$name.log
    timeout 900 "$bin" --autopilot "$out/$lesson-$name.out" --boot --school "$lesson" "$flag" "$car" \
        --steps 150000 --data "$RBR_DATA" --ngp-physics "$RBR_NGP_PHYSICS" >"$log" 2>&1 || true
    local verdict ended stopped
    verdict=$(grep -o 'verdict [0-9]' "$log" | head -1 | cut -d' ' -f2)
    ended=$(grep -o 'the race ended: [A-Za-z]*, EndRace { time: [0-9.]*' "$log" | head -1 |
        sed 's/the race ended: \([A-Za-z]*\), EndRace { time: \(.*\)/\1 at \2 s/')
    stopped=$(grep -o 'ended by .*' "$log" | head -1 | sed 's/ended by //')
    printf '%2s  %-40s  %-7s  %-22s  %s\n' "$lesson" "$name" "${verdict:--}" "${ended:-no end}" "$stopped"
}
export -f drive
export bin out RBR_DATA RBR_NGP_PHYSICS

printf '%2s  %-40s  %-7s  %-22s  %s\n' "#" "car" "verdict" "race end" "drive stopped by"
for lesson in ${lessons//,/ }; do
    for car in ${cars//,/ }; do
        printf '%s\t%s\n' "$lesson" "$car"
    done
done | xargs -P "$jobs" -L1 -d '\n' bash -c 'IFS=$'"'"'\t'"'"' read -r l c <<<"$0"; drive "$l" "$c"' |
    sort -n -k1,1 -s
echo "logs: $out"
