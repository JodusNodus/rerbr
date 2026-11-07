#!/usr/bin/env bash
# Car pace sweep without the RallySimFans install: steady-state grip and gravel braking.
#
# Drives every car of an added-cars folder (default ~/.local/share/rerbr/cars,
# the gvrc.de NGP packs: `cars.ini` names each pack folder with its display
# name, group and drive, the model under `Cars/`) two ways, on the retail
# 1.02 stages with NGP 7.4.778, and analyses each trace into one CSV row
# (harness/car_pace.py):
#
# - grip: a steady-state circle (radius 40 m) on the map-85 rally-school
#   tarmac with the target speed ramping up (`--circle -260,120,40`): the
#   first latched hold's fastest held speed and v^2/R grip. No 100 m circle:
#   no surveyed venue has a 200 m clear flat paved disc (map 85 is fenced
#   into smaller compounds; 481/Bikernieki/Slovakia-Ring discs sit hundreds
#   of metres from spawn through obstacles; Sosnova's files are encrypted).
# - gravel braking: flat-out standstill starts on map 61's start straight
#   (gravel, dry) with full braking from 100 km/h (`--brake-from 27.78`):
#   stopping distance and mean deceleration. The straight is flat and
#   straight past the stop for the whole range (the slowest pilot triggers
#   at ~330 m and stops ~390 m out; the first bend arrives past 500 m);
#   map 51's straight bends through the stop (the car brakes 2-3 m off the
#   line), map 21's ends before the slowest cars reach 100 km/h.
#
# What the sweep does NOT measure, and why no retail stage can host it:
#
# - pace: a flat-out standing start needs ~600 m of clear tarmac straight
#   (RSF 504's kink arrives ~600 m out, and even that truncates v20/v30).
#   The longest retail tarmac straight from a stage start is map 45's
#   (~185 m of flat-out window for a WRC car, then the corner): the Fiesta
#   WRC 2019 reaches 138.6 km/h there (no t_0_140; RSF 504: 7.07 s), slow
#   cars never reach 100 km/h. All six tarmac maps were driven with a WRC
#   car (windows 29-185 m, map 45 the longest).
# - tarmac braking: the same missing straight. From 150 km/h no car reaches
#   the trigger before map 45's corner (the Fiesta and the Audi S1 both end
#   at ~139-147 km/h); from 100 km/h the fast cars stop cleanly but the slow
#   ones never reach the trigger.
# - gravel circle: needs a compacted-gravel open area of ~100 m across
#   (RSF 345's start area: the Fiesta slides at 61.7 km/h, 0.75 g). The
#   retail school compounds are either fenced or blocked at spawn (maps
#   74-83, 87: never latch), deep loose gravel that bogs the car (maps 84,
#   88, 89: the Fiesta holds 35-51 km/h at 0.24-0.51 g), or tarmac ground
#   (map 72 shares map 85's compound and holds 0.86 g even on gravel tyres).
#
#   scripts/car-pace-sweep.sh [-o DIR] [-j JOBS] [-c CARS]
#
# DIR (default /tmp/car-pace-retail) holds <car>.{out,trace,log} plus the
# .circ and .gbrk100 variants, cars.tsv, cars.csv and meta.txt. CARS is the
# added-cars folder (default ~/.local/share/rerbr/cars, read-only). Needs
# dev.env's RBR_DATA and RBR_NGP_DLL (NGP 7.4.778's NGP.dll, with the
# 7.4.778 physics.rbz beside it, or RBR_NGP_PHYSICS_778 pointing at it).
# Needs no RBR_RSF and no RBR_NGP_PHYSICS (the dev archive has RSF cars in
# its slots). The run is bit-reproducible; rerunning overwrites DIR.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=dev-env.sh
source "$root/scripts/dev-env.sh"

out=/tmp/car-pace-retail
jobs=$(($(nproc) / 2))
cars_dir=~/.local/share/rerbr/cars
while getopts "o:j:c:h" opt; do
    case $opt in
    o) out=$OPTARG ;;
    j) jobs=$OPTARG ;;
    c) cars_dir=$OPTARG ;;
    *)
        sed -n '2,52p' "$0"
        exit 2
        ;;
    esac
done
: "${RBR_DATA:?RBR_DATA unset (dev.env)}" "${RBR_NGP_DLL:?RBR_NGP_DLL unset (the NGP 7.4.778 NGP.dll)}"
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
# with the model from models.tsv); $2 is the run suffix (.circ/.gbrk100)
# and the rest are the standalone arguments before the car flags.
drive() {
    local line=$1 suffix=$2
    shift 2
    local car section model
    IFS=$'\t' read -r car _ section _ _ _ <<<"$line"
    model=$(awk -F'\t' -v c="$car" '$1 == c { print $2 }' "$out/models.tsv")
    timeout 900 env -u RBR_RSF "$bin" --autopilot "$out/$car$suffix.out" --boot "$@" \
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
export bin out RBR_DATA ngp_physics cars_dir
export RBR_NGP_DLL

# Phase 1: steady-state circles on the map-85 school tarmac.
xargs -P "$jobs" -L1 -d '\n' bash -c 'drive "$0" ".circ" --stage track-85_M --steps 32000 --top-speed 60 --circle -260,120,40' <"$out/cars.tsv" | sort

# Phase 2: gravel brake-100 runs on map 61's start straight (dry gravel).
# 36000 steps: two snappy RWD Group B cars spin at launch, beach, relaunch
# and trigger past step 26000; everyone else stops long before the limit.
xargs -P "$jobs" -L1 -d '\n' bash -c 'drive "$0" ".gbrk100" --stage track-61_M --steps 36000 --top-speed 80 --flat-out --brake-from 27.78' <"$out/cars.tsv" | sort

python3 "$root/harness/car_pace.py" --manifest "$out/cars.tsv" --traces "$out" --csv "$out/cars.csv" \
    --surface tarmac --stage "dry tarmac circle (map 85) + dry gravel brakes (map 61)"
{
    echo "rbr-standalone: $("$bin" --version 2>/dev/null || echo ?)"
    echo "git: $(git -C "$root" rev-parse --short HEAD) $(git -C "$root" status --short | tr '\n' ' ')"
    echo "date: $(date -u +%FT%TZ)"
    # shellcheck disable=SC2016
    echo 'grip: --autopilot OUT --boot --stage track-85_M (--car-pack DIR --car-model PATH) --steps 32000 --top-speed 60 --circle -260,120,40 --trace-speed TRACE'
    # shellcheck disable=SC2016
    echo 'gravel braking: --autopilot OUT --boot --stage track-61_M (--car-pack DIR --car-model PATH) --steps 36000 --top-speed 80 --flat-out --brake-from 27.78 --trace-speed TRACE'
    echo "stages: retail map 85 (tarmac circle) and retail map 61 (gravel brakes); no RSF install (added-cars folder $cars_dir, NGP 7.4.778)"
} >"$out/meta.txt"
rm -f "$out"/*.out
echo "csv: $out/cars.csv"
