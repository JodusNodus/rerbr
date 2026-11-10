"""pad_start.py: flat-out standing starts on the flat pad, per car, surface and physics.

  python3 -I -B harness/pad_start.py [--cars Ford_Fiesta_Rally2_ngp7] [--surfaces tarmac,gravel]
      [--physics ngp,experimental] [--slips none,0.05,0.1,0.15,0.2,0.3] [--all] [--jobs 6] [--bin PATH]

Each run is `rbr-standalone --autopilot OUT --boot --stage pad-<surface> --rsf-car CAR --physics P --flat-out
[--drive-slip K --clutch-help off] --top-speed 80 --steps 26000 --trace-speed`: a flat-out standing start on the
generated pad, timed with `harness/car_pace.py`'s `load_trace` and `analyse` (the launch is the first move over
0.5 m/s). `none` is the clutch dump at full throttle (clutch help on, as the plain autopilot starts); a slip target K
is the driver's best start: the clutch slipped so the most-slipping wheel's drive slip holds near K with the engine
at its power peak, then the throttle holding K (`rbr-standalone`'s `autopilot::start`). Both shift up where the next
gear pulls harder. Prints one row per car x surface x physics: the run with the lowest 0-100 km/h (0-60 when the car
never reaches 100) with its slip target, and the dump's 0-100; `--all` prints every run.
target/release/rbr-standalone must be built; RBR_DATA and RBR_NGP_PHYSICS come from dev.env."""

import argparse
import concurrent.futures
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import dev_env  # noqa: E402,F401  (dev.env: RBR_DATA, RBR_NGP_PHYSICS)
from car_pace import analyse, load_trace  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(REPO, "target", "release", "rbr-standalone")
CARS = "Ford_Fiesta_Rally2_ngp7"
SURFACES = "tarmac,gravel"
PHYSICS = "ngp,experimental"
SLIPS = "none,0.05,0.1,0.15,0.2,0.3"


def runs_for(slips):
    """The start's runs (name, flags): the clutch dump (`none`) and one clutch launch per slip target."""
    runs = []
    for s in slips.split(","):
        if not s:
            continue
        runs.append(("none", []) if s == "none" else (f"k{s}", ["--drive-slip", s, "--clutch-help", "off"]))
    return runs


def run_one(args, car, surface, physics, name, flags):
    """One flat-out start; its (car, surface, physics, slip run) and the pace metrics."""
    tag = f"{car}-{surface}-{physics}-{name}".replace("/", "_")
    out = os.path.join(args.tmp, tag + ".out")
    trace = os.path.join(args.tmp, tag + ".trace")
    cmd = [args.bin, "--autopilot", out, "--boot", "--stage", f"pad-{surface}", "--rsf-car", car,
           "--physics", physics, "--flat-out", *flags, "--top-speed", "80", "--steps", "26000",
           "--trace-speed", trace, "--data", os.environ["RBR_DATA"],
           "--ngp-physics", os.environ["RBR_NGP_PHYSICS"]]
    with open(os.path.join(args.tmp, tag + ".log"), "w") as log:
        code = subprocess.run(cmd, stdout=log, stderr=subprocess.STDOUT).returncode
    if os.path.exists(out):
        os.remove(out)
    if code or not os.path.exists(trace):
        return (car, surface, physics, name, {"ok": False, "reason": f"exit {code}"})
    result = analyse(load_trace(trace))
    if not args.keep_traces:
        os.remove(trace)
    return (car, surface, physics, name, result)


def best_of(results):
    """The winning run: the lowest 0-100 km/h, or 0-60 when no run reaches 100."""
    ok = [r for r in results if r[4].get("ok")]
    with100 = [r for r in ok if r[4].get("t100") is not None]
    ranked = with100 or [r for r in ok if r[4].get("t60") is not None]
    key = (lambda r: r[4]["t100"]) if with100 else (lambda r: r[4]["t60"])
    return min(ranked, key=key) if ranked else None


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--cars", default=CARS)
    parser.add_argument("--surfaces", default=SURFACES)
    parser.add_argument("--physics", default=PHYSICS)
    parser.add_argument("--slips", default=SLIPS, help="drive-slip targets K (none: the clutch dump)")
    parser.add_argument("--all", action="store_true", help="print every run, not just each group's best")
    parser.add_argument("--jobs", type=int, default=6)
    parser.add_argument("--tmp", default="/tmp/pad-start")
    parser.add_argument("--keep-traces", action="store_true")
    parser.add_argument("--bin", default=BIN)
    args = parser.parse_args()
    os.makedirs(args.tmp, exist_ok=True)
    runs = runs_for(args.slips)
    jobs = [(c, s, p, name, flags) for c in args.cars.split(",") for s in args.surfaces.split(",")
            for p in args.physics.split(",") for name, flags in runs]
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        results = list(pool.map(lambda j: run_one(args, *j), jobs))
    print(f"{'car':24} {'surface':8} {'physics':19} {'0-60':>6} {'0-100':>6} {'0-140':>6} {'slip':6} {'dump':>6}")
    groups = [(c, s, p) for c in args.cars.split(",") for s in args.surfaces.split(",")
              for p in args.physics.split(",")]
    fmt = lambda v: "-" if v is None else f"{v:.2f}"  # noqa: E731
    for car, surface, physics in groups:
        group = [r for r in results if r[:3] == (car, surface, physics)]
        show = group if args.all else ([best_of(group)] if best_of(group) else group[:1])
        dump = next((r[4].get("t100") for r in group if r[3] == "none" and r[4].get("ok")), None)
        for car, surface, physics, name, r in show:
            if not r.get("ok"):
                print(f"{car:24} {surface:8} {physics:19} {r.get('reason', 'failed')} {name}")
                continue
            print(f"{car:24} {surface:8} {physics:19} {fmt(r.get('t60')):>6} {fmt(r.get('t100')):>6} "
                  f"{fmt(r.get('t140')):>6} {name:6} {fmt(dump):>6}")


if __name__ == "__main__":
    main()
