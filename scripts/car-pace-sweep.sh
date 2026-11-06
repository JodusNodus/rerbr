#!/usr/bin/env bash
# Straight-line pace sweep: flat-out standing starts for pace calibration.
#
# Drives every installed car (the 8 archive slots and every RSF pack with its
# physics installed) from a standing start on the start straight of RSF stage
# 504 (Witoszow - Zloty Las, tarmac, dry), full throttle without braking
# (`rbr-standalone --autopilot --flat-out --trace-speed`), and analyses each
# trace into one CSV row (harness/car_pace.py): 0-60/100/140/160 km/h times,
# speeds after 10/20/30 s, top speed and distance, all inside the flat-out
# window (launch to first brake, slide or corner).
#
#   scripts/car-pace-sweep.sh [-o DIR] [-j JOBS]
#
# DIR (default /tmp/car-pace) holds <car>.{out,trace,log}, cars.tsv, cars.csv
# and meta.txt. Needs dev.env's RBR_DATA, RBR_NGP_PHYSICS and RBR_RSF.
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
        sed -n '2,17p' "$0"
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

# One drive: manifest line "car kind key ..." (kind slot takes --car, else --rsf-car).
drive() {
    local line=$1 car kind key
    IFS=$'\t' read -r car kind key _ _ _ <<<"$line"
    local flag=--car
    [[ $kind == rsf ]] && flag=--rsf-car
    timeout 900 "$bin" --autopilot "$out/$car.out" --boot --rsf-stage 504 "$flag" "$key" \
        --steps 26000 --top-speed 80 --flat-out --trace-speed "$out/$car.trace" \
        --data "$RBR_DATA" --ngp-physics "$RBR_NGP_PHYSICS" >"$out/$car.log" 2>&1 || true
    if [[ -f $out/$car.trace ]]; then
        echo "$car: $(grep -c '' "$out/$car.trace") trace lines, $(tail -n 1 "$out/$car.log")"
    else
        echo "$car: NO TRACE (see $car.log)"
    fi
}
export -f drive
export bin out RBR_DATA RBR_NGP_PHYSICS

xargs -P "$jobs" -L1 -d '\n' bash -c 'drive "$0"' <"$out/cars.tsv" | sort

python3 "$root/harness/car_pace.py" --manifest "$out/cars.tsv" --traces "$out" --csv "$out/cars.csv"
{
    echo "rbr-standalone: $("$bin" --version 2>/dev/null || echo ?)"
    echo "git: $(git -C "$root" rev-parse --short HEAD) $(git -C "$root" status --short | tr '\n' ' ')"
    echo "date: $(date -u +%FT%TZ)"
    # shellcheck disable=SC2016
    echo 'command: --autopilot OUT --boot --rsf-stage 504 (--car N | --rsf-car PACK) --steps 26000 --top-speed 80 --flat-out --trace-speed TRACE'
    echo "stage: RSF 504 Witoszow-Zloty-Las variant O (tarmac, dry, good weather, default tyre, game-default aids)"
} >"$out/meta.txt"
rm -f "$out"/*.out
echo "csv: $out/cars.csv"
