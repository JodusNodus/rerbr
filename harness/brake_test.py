"""brake_test.py: locked wheels against threshold braking, one car on one stage, through the Rust autopilot.

  python3 -I -B harness/brake_test.py --rsf-stage 345 --rsf-car Ford_Fiesta_Rally2_ngp7 --from 27.78 --out DIR
      [--physics ngp|experimental|experimental-brush] [--slip 0.1,0.15,0.2,0.3] [--steps N] [--top-speed M/S]
      [--jobs N] [--stage NAME] [--clutch-out] [-- EXTRA rbr-standalone ARGS]

Each run is `rbr-standalone --autopilot --boot --flat-out --brake-from M/S --trace-speed`: the autopilot drives the
stage flat out and brakes once the speed is reached (pick a stage whose straight it reaches the speed on). One run
holds the full pedal (`--brake-pedal 1`, locked wheels); one per slip target K threshold-brakes (`--brake-slip K`:
one pedal modulated so the most-slipping wheel holds K, a driver's threshold braking, not a per-wheel ABS). Every run
brakes declutched (`--brake-declutch --clutch-help off`: clutch in, no shifts; NGP's clutch help would set the clutch
from the engine speed in place of the pedal), so the idling engine does not turn the braked wheels; `--clutch-out`
brakes in gear with clutch help on, as the plain brake test does (the autopilot downshifts, and on gravel the idling
engine drives the wheels against the brake). For each run it prints the stopping distance, time and mean deceleration
(car_pace.analyse_brake: brake onset to the stop, crash stops refused) and, while braking over 3 m/s past the first
tenth, the most-slipping wheel's slip (mean and spread), the wheels' mean rim speed and the deceleration through
32, 64 and 100 km/h (±1.5 m/s, TRL PPR496's speeds: a locked run's friction at that sliding speed, aero drag
included); last the locked distance over the best threshold distance. Traces and logs go to DIR (<run>.trace, <run>.log).
target/release/rbr-standalone must be built (`cargo build --release -p rbr-standalone`); RBR_DATA, RBR_RSF and
RBR_NGP_PHYSICS come from dev.env."""
import argparse
import concurrent.futures
import os
import statistics
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import dev_env  # noqa: E402,F401  (dev.env: RBR_DATA, RBR_RSF, RBR_NGP_PHYSICS)
import car_pace  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(REPO, "target", "release", "rbr-standalone")
G = 9.80665
SLIP_FROM = 3.0  # m/s: the slip statistics stop here (the ratio is noise near the stop)
BANDS = (32 / 3.6, 64 / 3.6, 100 / 3.6)  # m/s: the deceleration at TRL PPR496's test speeds (32, 64, 100 km/h)
BAND_HALF = 1.5  # m/s either side of a band's centre


def command(args, name, brake):
    """The autopilot run `name` with `brake` (the brake test's own flags)."""
    stage = ["--rsf-stage", str(args.rsf_stage)] if args.rsf_stage is not None else ["--stage", args.stage]
    return ([BIN, "--autopilot", os.path.join(args.out, name + ".out"), "--boot", *stage,
             "--rsf-car", args.rsf_car, "--physics", args.physics, "--steps", str(args.steps),
             "--top-speed", str(args.top_speed), "--flat-out", "--brake-from", str(getattr(args, "from")), *brake,
             *([] if args.clutch_out else ["--brake-declutch", "--clutch-help", "off"]),
             "--trace-speed", os.path.join(args.out, name + ".trace"), "--data", os.environ["RBR_DATA"],
             "--ngp-physics", os.environ["RBR_NGP_PHYSICS"], *args.extra])


def run(cmd, log):
    with open(log, "w") as f:
        return subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT).returncode


def slip_stats(rows, onset):
    """From the brake onset down to SLIP_FROM, past the first tenth: the most-slipping wheel's slip 1 - |wR|/v (mean,
    spread) and the four wheels' mean rim speed |wR| (m/s; near 0 locked)."""
    window = []
    for row in rows[onset:]:
        if row["v"] < SLIP_FROM:
            break
        window.append(row)
    body = window[len(window) // 10:]
    if not body:
        return None, None, None
    slips = [max((row["v"] - w) / row["v"] for w in row["wheels"]) for row in body]
    rim = statistics.mean(statistics.mean(row["wheels"]) for row in body)
    return statistics.mean(slips), statistics.pstdev(slips), rim


def band_decel(rows, onset, centre, half=BAND_HALF):
    """Mean deceleration (m/s^2) while the speed falls from centre + half to centre - half past the brake onset (the
    car's friction at that sliding speed when locked), or None if the run does not cross the band."""
    hi = next((i for i in range(onset, len(rows)) if rows[i]["v"] <= centre + half), None)
    lo = next((i for i in range(hi or onset, len(rows)) if rows[i]["v"] <= centre - half), None) if hi else None
    if hi is None or lo is None or hi == onset or rows[lo]["t"] <= rows[hi]["t"]:
        return None
    return (rows[hi]["v"] - rows[lo]["v"]) / (rows[lo]["t"] - rows[hi]["t"])


def analyse(path, frm):
    rows = car_pace.load_trace(path)
    result = car_pace.analyse_brake(rows, frm)
    if result["ok"] and "wheels" in rows[0]:
        onset = next(i for i in range(result["launch"], len(rows)) if rows[i]["brk"] > car_pace.BRK_ONSET)
        result["slip"] = slip_stats(rows, onset)
        result["bands"] = [band_decel(rows, onset, c) for c in BANDS]
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    stage = parser.add_mutually_exclusive_group(required=True)
    stage.add_argument("--rsf-stage", type=int)
    stage.add_argument("--stage")
    parser.add_argument("--rsf-car", required=True)
    parser.add_argument("--physics", default="ngp")
    parser.add_argument("--from", type=float, required=True, help="the braking speed, m/s")
    parser.add_argument("--slip", default="0.1,0.15,0.2,0.3", help="threshold braking's slip targets K")
    parser.add_argument("--steps", type=int, default=30000)
    parser.add_argument("--top-speed", type=float, default=80.0)
    parser.add_argument("--jobs", type=int, default=5)
    parser.add_argument("--clutch-out", action="store_true", help="brake in gear, without --brake-declutch")
    parser.add_argument("--out", required=True)
    parser.add_argument("extra", nargs="*", help="after --: more rbr-standalone arguments")
    args = parser.parse_args()
    if not os.path.exists(BIN):
        sys.exit(f"{BIN}: build it first (cargo build --release -p rbr-standalone)")
    os.makedirs(args.out, exist_ok=True)
    frm = getattr(args, "from")
    runs = [("locked", ["--brake-pedal", "1"])]
    runs += [(f"k{k}", ["--brake-slip", k]) for k in args.slip.split(",")]
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        codes = list(pool.map(lambda r: run(command(args, *r), os.path.join(args.out, r[0] + ".log")), runs))
    clutch = "clutch out" if args.clutch_out else "declutched"
    print(f"{args.rsf_car} on {args.stage or args.rsf_stage}, {args.physics}, {clutch}, "
          f"from {frm:.2f} m/s ({frm * 3.6:.0f} km/h)")
    print(f"{'run':8} {'dist m':>7} {'time s':>7} {'decel g':>8} {'slip':>12} {'rim m/s':>7}  g at 32/64/100 km/h")
    best = None
    locked = None
    for (name, _), code in zip(runs, codes):
        trace = os.path.join(args.out, name + ".trace")
        result = analyse(trace, frm) if os.path.exists(trace) else {"ok": False, "reason": f"no trace (exit {code})"}
        if not result["ok"]:
            print(f"{name:8} {result['reason']}")
            continue
        mean, sd, rim = result.get("slip", (None, None, None))
        slip, rim = (f"{mean:.3f}±{sd:.3f}", f"{rim:.2f}") if mean is not None else ("-", "-")
        bands = " ".join(f"{b / G:.3f}" if b else "-" for b in result.get("bands", ()))
        print(f"{name:8} {result['dist']:7.1f} {result['time']:7.2f} {result['decel'] / G:8.3f} {slip:>12} {rim:>7}  "
              f"{bands}")
        if name == "locked":
            locked = result
        elif best is None or result["dist"] < best[1]["dist"]:
            best = (name, result)
    if locked and best:
        print(f"locked / best threshold ({best[0]}): {locked['dist'] / best[1]['dist']:.3f}")


if __name__ == "__main__":
    main()
