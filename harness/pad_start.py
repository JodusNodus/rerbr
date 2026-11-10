"""pad_start.py: flat-out standing starts on the flat pad, per car, surface and physics.

  python3 -I -B harness/pad_start.py [--cars Ford_Fiesta_Rally2_ngp7] [--surfaces tarmac,gravel]
      [--physics ngp,experimental] [--jobs 6] [--bin PATH]

Each run is `rbr-standalone --autopilot OUT --boot --stage pad-<surface> --rsf-car CAR --physics P --flat-out
--top-speed 80 --steps 26000 --trace-speed`: a flat-out standing start on the generated pad, timed with
`harness/car_pace.py`'s `load_trace` and `analyse` (the launch is the first move over 0.5 m/s). Prints 0-60, 0-100
and 0-140 km/h per run.
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


def run_one(args, car, surface, physics):
    """One flat-out start; its (car, surface, physics) and the pace metrics."""
    tag = f"{car}-{surface}-{physics}".replace("/", "_")
    out = os.path.join(args.tmp, tag + ".out")
    trace = os.path.join(args.tmp, tag + ".trace")
    cmd = [args.bin, "--autopilot", out, "--boot", "--stage", f"pad-{surface}", "--rsf-car", car,
           "--physics", physics, "--flat-out", "--top-speed", "80", "--steps", "26000",
           "--trace-speed", trace, "--data", os.environ["RBR_DATA"],
           "--ngp-physics", os.environ["RBR_NGP_PHYSICS"]]
    with open(os.path.join(args.tmp, tag + ".log"), "w") as log:
        code = subprocess.run(cmd, stdout=log, stderr=subprocess.STDOUT).returncode
    if os.path.exists(out):
        os.remove(out)
    if code or not os.path.exists(trace):
        return (car, surface, physics, {"ok": False, "reason": f"exit {code}"})
    result = analyse(load_trace(trace))
    if not args.keep_traces:
        os.remove(trace)
    return (car, surface, physics, result)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--cars", default=CARS)
    parser.add_argument("--surfaces", default=SURFACES)
    parser.add_argument("--physics", default=PHYSICS)
    parser.add_argument("--jobs", type=int, default=6)
    parser.add_argument("--tmp", default="/tmp/pad-start")
    parser.add_argument("--keep-traces", action="store_true")
    parser.add_argument("--bin", default=BIN)
    args = parser.parse_args()
    os.makedirs(args.tmp, exist_ok=True)
    jobs = [(c, s, p) for c in args.cars.split(",") for s in args.surfaces.split(",")
            for p in args.physics.split(",")]
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        results = list(pool.map(lambda j: run_one(args, *j), jobs))
    print(f"{'car':24} {'surface':8} {'physics':19} {'0-60':>6} {'0-100':>6} {'0-140':>6}")
    for car, surface, physics, r in results:
        if not r.get("ok"):
            print(f"{car:24} {surface:8} {physics:19} {r.get('reason', 'failed')}")
            continue
        fmt = lambda v: "-" if v is None else f"{v:.2f}"  # noqa: E731
        print(f"{car:24} {surface:8} {physics:19} {fmt(r.get('t60')):>6} {fmt(r.get('t100')):>6} "
              f"{fmt(r.get('t140')):>6}")


if __name__ == "__main__":
    main()
