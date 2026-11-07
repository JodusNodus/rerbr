#!/usr/bin/env bash
# Car pace sweep: straight-line pace, steady-state grip and braking.
#
# Drives every car of an added-cars folder (default ~/.local/share/rerbr/cars,
# the gvrc.de NGP packs: `cars.ini` names each pack folder with its display
# name, group and drive, the model under `Cars/`) with `--car-pack`/
# `--car-model` under NGP 7.4.778 (no archive slots), all on dry tarmac with
# default tyres and the game-default aids, and analyses each trace into one
# CSV row (harness/car_pace.py). The stages come from the RallySimFans
# install (RBR_RSF): only its stage files, never its cars.
#
# - pace: a flat-out standing start on the start straight of RSF stage 504
#   (Witoszow - Zloty Las, variant O), `--flat-out --trace-speed`: 0-60/100/
#   140/160 km/h times, speeds after 10/20/30 s, top speed and distance, all
#   inside the flat-out window (launch to first brake, slide or corner).
# - grip: a steady-state circle (radius 40 m) on the map-85 rally-school
#   tarmac with the target speed ramping up (`--circle -260,120,40`): the
#   first latched hold's fastest held speed and v^2/R grip. No 100 m circle:
#   no surveyed venue has a 200 m clear flat paved disc (map 85 is fenced
#   into smaller compounds; 481/Bikernieki/Slovakia-Ring discs sit hundreds
#   of metres from spawn through obstacles; Sosnova's files are encrypted).
# - braking: flat-out standstill starts on the 1 km start straight of RSF
#   stage 568 (Graciosa I, variant M, tarmac, dry) then full braking from
#   100 km/h (`--brake-from 27.78`) and, for cars whose pace run reaches it,
#   from 150 km/h (`--brake-from 41.67`): stopping distance and mean
#   deceleration. The 504 straight cannot host the 150 km/h stop (its kink
#   and roadside trees arrive 300 m past launch, so slower accelerators
#   crash into them while still fast); 568's straight is clear for 1 km.
#   A crash-stop or an unreached trigger leaves those columns empty.
# - gravel: the same circle (radius 40 m) on the open start area of RSF stage
#   345 (Azov II, variant M, gravel, dry; `--circle 1051,-166,40 --circle-ramp
#   0.4`) and flat-out standstill starts on 345's 970 m start straight with
#   full braking from 100 km/h: the `*_gravel` columns. The gentle ramp is the
#   repeatable quasi-steady one (the default 0.5 is a knife-edge on gravel:
#   neutral cars blow through to 65+ pushing, loose ones saw at 52-56). The
#   113/390 rally-school compounds beach
#   the car in deep loose gravel (unmapped bogs stop it even in first gear at
#   full revs); 345's compacted start area holds every pilot car to a slide.
#
# The retail 1.02 stages cannot host these runs, so the RSF install stays a
# stage source: the longest retail tarmac straight from a stage start is map
# 45's (~185 m of flat-out window for a WRC car, then the corner — the
# Fiesta WRC 2019 reaches 138.6 km/h there and never triggers the 150 km/h
# braking; slow cars never reach 100), no school compound replicates 345's
# compacted gravel (fenced or blocked at spawn, boggy at 0.24-0.51 g, or
# tarmac ground), and rbr-standalone has no start-point option.
#
#   scripts/car-pace-sweep.sh [-o DIR] [-j JOBS] [-c CARS]
#
# DIR (default /tmp/car-pace-all) holds <car>.{out,trace,log} plus the .circ,
# .brk100, .brk150, .gcirc and .gbrk100 variants, cars.tsv, cars.csv and
# meta.txt. CARS is the added-cars folder (default ~/.local/share/rerbr/cars,
# read-only). Needs dev.env's RBR_DATA, RBR_NGP_DLL (NGP 7.4.778's NGP.dll,
# with the 7.4.778 physics.rbz beside it, or RBR_NGP_PHYSICS_778 pointing at
# it) and RBR_RSF (its stage files only). The run is bit-reproducible;
# rerunning overwrites DIR.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=dev-env.sh
source "$root/scripts/dev-env.sh"

out=/tmp/car-pace-all
jobs=$(($(nproc) / 2))
cars_dir=~/.local/share/rerbr/cars
while getopts "o:j:c:h" opt; do
    case $opt in
    o) out=$OPTARG ;;
    j) jobs=$OPTARG ;;
    c) cars_dir=$OPTARG ;;
    *)
        sed -n '2,51p' "$0"
        exit 2
        ;;
    esac
done
: "${RBR_DATA:?RBR_DATA unset (dev.env)}" "${RBR_NGP_DLL:?RBR_NGP_DLL unset (the NGP 7.4.778 NGP.dll)}" \
  "${RBR_RSF:?RBR_RSF unset (dev.env; its stage files only)}"
ngp_physics=${RBR_NGP_PHYSICS_778:-${RBR_NGP_DLL%/*}/physics.rbz}
[[ -f $ngp_physics ]] || {
    echo "NGP physics archive not found: $ngp_physics (set RBR_NGP_PHYSICS_778)" >&2
    exit 2
}
[[ -f $cars_dir/cars.ini ]] || {
    echo "added-cars folder has no cars.ini: $cars_dir (pass -c DIR)" >&2
    exit 2
}
mkdir -p "$out"

bin=$root/target/release/rbr-standalone
[[ -x $bin ]] || cargo build --release -q -p rbr-standalone --bin rbr-standalone --manifest-path "$root/Cargo.toml"

# The car list: every pack of the added-cars folder (section, display name,
# group, drive and revision from cars.ini, the model under Cars/).
python3 - "$cars_dir" >"$out/cars.tsv" <<'EOF'
import configparser, re, sys
ini = configparser.ConfigParser()
ini.read(f"{sys.argv[1]}/cars.ini")
for section in sorted(ini.sections()):
    name = ini[section].get("name", section)
    group = ini[section].get("group", "")
    drive = ini[section].get("drive", "")
    revision = ini[section].get("revision", "")
    extra = " | ".join(part for part in (drive, f"rev {revision}" if revision else "") if part)
    safe = re.sub(r"[^A-Za-z0-9_.-]+", "_", section)
    model = ini[section]["model"]
    print(f"{safe}\tpack\t{section}\t{name}\t{group}\t{extra}\t{model}")
EOF
cut -f1,7 "$out/cars.tsv" >"$out/models.tsv"
cut -f1-6 "$out/cars.tsv" >"$out/manifest.tsv"
mv "$out/manifest.tsv" "$out/cars.tsv"
echo "cars: $(wc -l <"$out/cars.tsv")"

# One drive: manifest line "car kind section ..." (pack takes --car-pack
# with the model from models.tsv); $2 is the run suffix (empty for pace,
# .circ/.brk100/.brk150/.gcirc/.gbrk100) and the rest are the standalone
# arguments before the car flags.
drive() {
    local line=$1 suffix=$2
    shift 2
    local car section model
    IFS=$'\t' read -r car _ section _ _ _ <<<"$line"
    model=$(awk -F'\t' -v c="$car" '$1 == c { print $2 }' "$out/models.tsv")
    timeout 900 "$bin" --autopilot "$out/$car$suffix.out" --boot "$@" \
        --car-pack "$cars_dir/rsfdata/cars/$section" --car-model "$cars_dir/Cars/$model.sgc" \
        --trace-speed "$out/$car$suffix.trace" --data "$RBR_DATA" \
        --ngp-physics "$ngp_physics" >"$out/$car$suffix.log" 2>&1 || true
    if [[ -f $out/$car$suffix.trace ]]; then
        echo "$car$suffix: $(grep -c '' "$out/$car$suffix.trace") trace lines, $(tail -n 1 "$out/$car$suffix.log")"
    else
        echo "$car$suffix: NO TRACE (see $car$suffix.log)"
    fi
}
export -f drive
export bin out RBR_DATA RBR_RSF ngp_physics cars_dir
export RBR_NGP_DLL

# Phase 1: flat-out pace runs (their vtop gates the brake-150 runs).
xargs -P "$jobs" -L1 -d '\n' bash -c 'drive "$0" "" --rsf-stage 504 --steps 26000 --top-speed 80 --flat-out' <"$out/cars.tsv" | sort

# Phase 2: steady-state circles and brake-100 runs (unconditional).
xargs -P "$jobs" -L1 -d '\n' bash -c 'drive "$0" ".circ" --stage track-85_M --steps 32000 --top-speed 60 --circle -260,120,40' <"$out/cars.tsv" | sort
xargs -P "$jobs" -L1 -d '\n' bash -c 'drive "$0" ".brk100" --rsf-stage 568 --steps 26000 --top-speed 80 --flat-out --brake-from 27.78' <"$out/cars.tsv" | sort

# Phase 3: brake-150 runs for cars whose pace run reaches 150 km/h.
python3 "$root/harness/car_pace.py" --manifest "$out/cars.tsv" --traces "$out" \
    --csv "$out/qualify.csv" --qualify >"$out/qualify.txt"
join -t $'\t' -1 1 -2 1 <(sort "$out/cars.tsv") <(sort "$out/qualify.txt") >"$out/cars150.tsv" || true
xargs -P "$jobs" -L1 -d '\n' bash -c 'drive "$0" ".brk150" --rsf-stage 568 --steps 26000 --top-speed 80 --flat-out --brake-from 41.67' <"$out/cars150.tsv" | sort

# Phases 4-5: gravel circle and gravel brake-100 runs (RSF 345, dry gravel).
xargs -P "$jobs" -L1 -d '\n' bash -c 'drive "$0" ".gcirc" --rsf-stage 345 --steps 32000 --top-speed 60 --circle 1051,-166,40 --circle-ramp 0.4' <"$out/cars.tsv" | sort
xargs -P "$jobs" -L1 -d '\n' bash -c 'drive "$0" ".gbrk100" --rsf-stage 345 --steps 26000 --top-speed 80 --flat-out --brake-from 27.78' <"$out/cars.tsv" | sort

python3 "$root/harness/car_pace.py" --manifest "$out/cars.tsv" --traces "$out" --csv "$out/cars.csv"
{
    echo "rbr-standalone: $("$bin" --version 2>/dev/null || echo ?)"
    echo "git: $(git -C "$root" rev-parse --short HEAD) $(git -C "$root" status --short | tr '\n' ' ')"
    echo "date: $(date -u +%FT%TZ)"
    # shellcheck disable=SC2016
    echo 'pace: --autopilot OUT --boot --rsf-stage 504 (--car-pack DIR --car-model PATH) --steps 26000 --top-speed 80 --flat-out --trace-speed TRACE'
    # shellcheck disable=SC2016
    echo 'grip: --autopilot OUT --boot --stage track-85_M (--car-pack DIR --car-model PATH) --steps 32000 --top-speed 60 --circle -260,120,40 --trace-speed TRACE'
    # shellcheck disable=SC2016
    echo 'braking: --autopilot OUT --boot --rsf-stage 568 (--car-pack DIR --car-model PATH) --steps 26000 --top-speed 80 --flat-out --brake-from 27.78 (100) / 41.67 (150, only if the pace vtop reaches 150) --trace-speed TRACE'
    # shellcheck disable=SC2016
    echo 'gravel: --autopilot OUT --boot --rsf-stage 345 (--car-pack DIR --car-model PATH) --steps 32000 --top-speed 60 --circle 1051,-166,40 --circle-ramp 0.4 --trace-speed TRACE + --steps 26000 --top-speed 80 --flat-out --brake-from 27.78 --trace-speed TRACE'
    echo "stages: RSF 504-O (pace), RSF 568-M (brakes), retail map 85 (circle), RSF 345-M (gravel circle + brakes); all dry, good weather, default tyre, game-default aids; cars from the added-cars folder $cars_dir under NGP 7.4.778, no archive slots"
} >"$out/meta.txt"
rm -f "$out"/*.out
rm -f "$out"/qualify.csv
rm -f "$out"/qualify.txt
rm -f "$out"/cars150.tsv
echo "csv: $out/cars.csv"
