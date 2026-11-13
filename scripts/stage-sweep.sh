#!/usr/bin/env bash
# Drives every RallySimFans stage of an install headless with the autopilot (`rbr-standalone --autopilot /dev/null
# --boot --rsf-stage ID`) and writes one tab-separated row a drive: whether the stage loads, how far the autopilot
# got, how the drive and the race ended, and the worst distance off and height over the driveline (a car through the
# ground shows as a large negative height). A check of the community-stage load path and of the physics on every
# installed stage, not of the autopilot: it gets stuck on some stages by itself. No input device is opened.
#
#   scripts/stage-sweep.sh [-s IDS] [-w WEATHERS] [-c CARS] [-n STEPS] [-j JOBS] [-o DIR]
#
# IDS: comma-separated stage ids (default every stage folder under $RBR_RSF/Maps). WEATHERS: comma-separated
# `--weather` values (default 0). CARS: comma-separated `--car` slots, `rsf:ID` RSF cars (`--rsf-car ID`) or RSF pack folders (default 5). STEPS: the step limit
# (default 1000000, about 23 minutes of driving). Resumable: a drive with a log in DIR is not run again. Uses
# $RBR_STANDALONE, else target/release/rbr-standalone (built if missing), and dev.env's RBR_DATA, RBR_RSF and RBR_NGP_PHYSICS. Results go to
# DIR/sweep.tsv (default a temporary folder), logs without the autopilot's progress lines to DIR/logs/.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=dev-env.sh
source "$root/scripts/dev-env.sh"

ids=
weathers=0
cars=5
steps=1000000
jobs=$(($(nproc) / 2))
out=
while getopts "s:w:c:n:j:o:h" opt; do
    case $opt in
    s) ids=$OPTARG ;;
    w) weathers=$OPTARG ;;
    c) cars=$OPTARG ;;
    n) steps=$OPTARG ;;
    j) jobs=$OPTARG ;;
    o) out=$OPTARG ;;
    *)
        sed -n '2,13p' "$0"
        exit 2
        ;;
    esac
done
: "${RBR_DATA:?RBR_DATA unset (dev.env)}" "${RBR_RSF:?RBR_RSF unset (dev.env)}"
: "${RBR_NGP_PHYSICS:?RBR_NGP_PHYSICS unset (dev.env)}"
out=${out:-$(mktemp -d -t stage-sweep.XXXXXX)}
mkdir -p "$out/logs"

bin=${RBR_STANDALONE:-$root/target/release/rbr-standalone}
[[ -x $bin ]] || cargo build --release -q -p rbr-standalone --bin rbr-standalone --manifest-path "$root/Cargo.toml"

if [[ -z $ids ]]; then
    ids=$(find "$RBR_RSF/Maps" -mindepth 1 -maxdepth 1 \( -type d -o -type l \) -printf '%f\n' |
        sed -n 's/^\([0-9]\+\)-.*/\1/p' | sort -n | paste -sd,)
fi

# One drive: stage $1, weather $2, car $3. Prints its row.
drive() {
    local id=$1 weather=$2 car=$3 flag=--car value=$3 log start secs rc
    [[ $car =~ ^[0-9]+$ ]] || flag=--rsf-car value=${car#rsf:}
    log=$out/logs/$id-w$weather.log
    [[ $car == 5 ]] || log=$out/logs/$id-w$weather-c$(tr '/:' '__' <<<"$car").log
    if [[ ! -f $log ]]; then
        start=$(date +%s)
        timeout 3600 "$bin" --autopilot /dev/null --boot --rsf-stage "$id" --rsf-root "$RBR_RSF" --weather "$weather" \
            "$flag" "$value" --steps "$steps" --data "$RBR_DATA" --ngp-physics "$RBR_NGP_PHYSICS" 2>&1 |
            awk '/^autopilot: step / {
                    n++; last = $0
                    if (match($0, /, [-0-9.]+ m off/)) { v = substr($0, RSTART + 2, RLENGTH - 8) + 0; if (v > off) off = v }
                    if (match($0, /, [-0-9.]+ m above/)) { v = substr($0, RSTART + 2, RLENGTH - 10) + 0; if (n == 1 || v < low) low = v }
                    if ($0 ~ /NaN|[^a-z]inf/) nan++
                    next }
                 { print }
                 END { print "sweep: last " last; printf "sweep: max_off %.1f min_above %.1f nan_lines %d\n", off, low, nan }' \
                >"$log.part"
        rc=${PIPESTATUS[0]}
        secs=$(($(date +%s) - start))
        echo "sweep: exit $rc secs $secs" >>"$log.part"
        mv "$log.part" "$log"
    fi
    awk -v id="$id" -v w="$weather" -v car="$car" '
        /^weather: / && match($0, /track-[0-9]+_[A-Z]/) { variant = substr($0, RSTART + RLENGTH - 1, 1) }
        /steps driven, finish/ {
            s = $0; sub(/.*steps driven, finish /, "", s); split(s, a, ", ended by "); finish = a[1]; ended = a[2] }
        /^rally: StageEnd/ {
            s = $0; sub(/.*cause: /, "", s); cause = s; sub(/,.*/, "", cause)
            t = $0; sub(/.*time: /, "", t); sub(/,.*/, "", t); rtime = t
            retired = ($0 ~ /retired: true/) ? "retired" : "" }
        /^sweep: last / && match($0, /at [0-9.]+ of [0-9.]+ m/) { s = substr($0, RSTART + 3, RLENGTH - 5); sub(/ of /, "/", s); dist = s }
        /^sweep: max_off / { off = $3; low = $5; nan = $7 }
        /^sweep: exit / { rc = $3; secs = $5 }
        /panicked at/ && panic == "" { panic = $0 }
        !/^(weather|autopilot|sweep|rally|usage|       |rbr-standalone: boot built)/ && !/steps driven, finish/ && other == "" { other = $0 }
        END {
            err = panic != "" ? panic : rc != 0 ? other : ""; gsub(/\t/, " ", err)
            printf "%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n", id, w, car, (variant == "" ? "-" : variant), rc,
                (ended == "" ? "-" : ended), (finish == "" ? "-" : finish), (cause == "" ? "-" : cause " " retired),
                (rtime == "" ? "-" : rtime), (dist == "" ? "-" : dist), off, low, nan, secs, (err == "" ? "-" : substr(err, 1, 300))
        }' "$log"
}
export -f drive
export bin out steps RBR_DATA RBR_RSF RBR_NGP_PHYSICS

printf 'id\tweather\tcar\tvariant\texit\tended_by\tfinish\trace_end\trace_time\tdistance_m\tmax_off\tmin_above\tnan_lines\tsecs\terror\n' \
    >"$out/sweep.tsv"
for id in ${ids//,/ }; do
    for w in ${weathers//,/ }; do
        for c in ${cars//,/ }; do
            printf '%s\t%s\t%s\n' "$id" "$w" "$c"
        done
    done
done | xargs -P "$jobs" -L1 -d '\n' bash -c 'IFS=$'"'"'\t'"'"' read -r i w c <<<"$0"; drive "$i" "$w" "$c"' |
    sort -n -k1,1 -k2,2 >>"$out/sweep.tsv"
echo "results: $out/sweep.tsv"
