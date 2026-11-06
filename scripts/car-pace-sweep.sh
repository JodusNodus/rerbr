#!/usr/bin/env bash
# Car pace sweep: straight-line pace, steady-state grip and braking.
#
# Drives every installed car (the 8 archive slots and every RSF pack with its
# physics installed) three ways, all on dry tarmac with default tyres and the
# game-default aids, and analyses each trace into one CSV row
# (harness/car_pace.py):
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
#
#   scripts/car-pace-sweep.sh [-o DIR] [-j JOBS]
#
# DIR (default /tmp/car-pace) holds <car>.{out,trace,log} plus the .circ,
# .brk100 and .brk150 variants, cars.tsv, cars.csv and meta.txt. Needs
# dev.env's RBR_DATA, RBR_NGP_PHYSICS and RBR_RSF.
# The run is bit-reproducible; rerunning overwrites DIR.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
# shellcheck source=dev-env.sh
source "$root/scripts/dev-env.sh"

out=/tmp/car-pace
jobs=$(($(nproc) / 2))
while getopts "o:j:h" opt; do
    case $opt in
    o) out=$OPTARG ;;
    j) jobs=$OPTARG ;;
    *)
        sed -n '2,31p' "$0"
        exit 2
        ;;
    esac
done
: "${RBR_DATA:?RBR_DATA unset (dev.env)}" "${RBR_NGP_PHYSICS:?RBR_NGP_PHYSICS unset (dev.env)}" \
  "${RBR_RSF:?RBR_RSF unset (dev.env)}"
mkdir -p "$out"

bin=$root/target/release/rbr-standalone
[[ -x $bin ]] || cargo build --release -q -p rbr-standalone --bin rbr-standalone --manifest-path "$root/Cargo.toml"

# The car list: the 8 archive slots, then every RSF cache car whose physics
# pack is installed (id, pack folder, display name, base group, specs).
python3 - "$RBR_RSF" "$RBR_DATA" >"$out/cars.tsv" <<'EOF'
import json, os, sys
rsf, data = sys.argv[1], sys.argv[2]
names = {}
with open(os.path.join(data, "Cars", "Cars.ini")) as handle:
    index = None
    for line in handle:
        line = line.strip()
        if line.startswith("[Car"):
            index = int(line[4:6])
        elif line.startswith("FileName") and index is not None:
            names[index] = line.split("=", 1)[1].strip().strip('"').replace("\\", "/").split("/")[-1]
for slot in range(8):
    print(f"slot-{slot}\tslot\t{slot}\t{names.get(slot, '?')}\t\t")
cache = os.path.join(rsf, "rsfdata", "cache")
cars = json.load(open(os.path.join(cache, "cars.json")))
groups = {g["id"]: g["name"] for g in json.load(open(os.path.join(cache, "cargroups.json")))}
specs = {r["car_id"]: r for r in json.load(open(os.path.join(cache, "cars_data.json")))}
installed = set(os.listdir(os.path.join(rsf, "rsfdata", "cars")))
for car in sorted(cars, key=lambda c: int(c["id"])):
    if car["path"] not in installed and car["path"] + ".zip" not in installed:
        continue
    spec = specs.get(car["id"], {})
    drive = spec.get("drive_train", "")
    power = spec.get("power", "").replace(" /", "/")
    weight = spec.get("weight", "")
    extra = " | ".join(part for part in (drive, power, weight) if part)
    print(f"{car['path']}\trsf\t{car['id']}\t{car['name']}\t{groups.get(car['base_group_id'], '')}\t{extra}")
EOF
echo "cars: $(wc -l <"$out/cars.tsv")"

# One drive: manifest line "car kind key ..." (kind slot takes --car, else
# --rsf-car); $2 is the run suffix (empty for pace, .circ/.brk100/.brk150)
# and the rest are the standalone arguments before the car flag.
drive() {
    local line=$1 suffix=$2
    shift 2
    local car kind key
    IFS=$'\t' read -r car kind key _ _ _ <<<"$line"
    local flag=--car
    [[ $kind == rsf ]] && flag=--rsf-car
    timeout 900 "$bin" --autopilot "$out/$car$suffix.out" --boot "$@" "$flag" "$key" \
        --trace-speed "$out/$car$suffix.trace" --data "$RBR_DATA" \
        --ngp-physics "$RBR_NGP_PHYSICS" >"$out/$car$suffix.log" 2>&1 || true
    if [[ -f $out/$car$suffix.trace ]]; then
        echo "$car$suffix: $(grep -c '' "$out/$car$suffix.trace") trace lines, $(tail -n 1 "$out/$car$suffix.log")"
    else
        echo "$car$suffix: NO TRACE (see $car$suffix.log)"
    fi
}
export -f drive
export bin out RBR_DATA RBR_NGP_PHYSICS

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

python3 "$root/harness/car_pace.py" --manifest "$out/cars.tsv" --traces "$out" --csv "$out/cars.csv"
{
    echo "rbr-standalone: $("$bin" --version 2>/dev/null || echo ?)"
    echo "git: $(git -C "$root" rev-parse --short HEAD) $(git -C "$root" status --short | tr '\n' ' ')"
    echo "date: $(date -u +%FT%TZ)"
    # shellcheck disable=SC2016
    echo 'pace: --autopilot OUT --boot --rsf-stage 504 (--car N | --rsf-car PACK) --steps 26000 --top-speed 80 --flat-out --trace-speed TRACE'
    # shellcheck disable=SC2016
    echo 'grip: --autopilot OUT --boot --stage track-85_M (--car N | --rsf-car PACK) --steps 32000 --top-speed 60 --circle -260,120,40 --trace-speed TRACE'
    # shellcheck disable=SC2016
    echo 'braking: --autopilot OUT --boot --rsf-stage 568 (--car N | --rsf-car PACK) --steps 26000 --top-speed 80 --flat-out --brake-from 27.78 (100) / 41.67 (150, only if the pace vtop reaches 150) --trace-speed TRACE'
    echo "stages: RSF 504-O (pace), RSF 568-M (brakes), retail map 85 (circle); all tarmac, dry, good weather, default tyre, game-default aids"
} >"$out/meta.txt"
rm -f "$out"/*.out
rm -f "$out"/qualify.csv
rm -f "$out"/qualify.txt
rm -f "$out"/cars150.tsv
echo "csv: $out/cars.csv"
