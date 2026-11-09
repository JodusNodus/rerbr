"""car_grip.py: the steady circle's car-level lateral g in the real telemetry's terms (notes/tyre-model.md "Car-level
grip in the same terms").

  python3 -I -B harness/car_grip.py --out DIR [--cars A,B] [--physics ngp,experimental-brush]
      [--surfaces gravel,snow,tarmac] [--radii 40] [--ramps R] [--extra "FLAGS" --tag T] [--jobs N] [--csv OUT]

Drives each car x physics x surface x radius x ramp on limit_pace.py's circle (SURFACES: gravel RSF 345, snow RSF 345
under material 101 with the snow tyre, tarmac map 85 under tarmac 37; the circle's centre with the radius given; the
ramp in m/s per s, the surface's own when not given: 0.4 on RSF 345, the autopilot's 0.5 on map 85), the traces to
DIR/<car>.<physics>.<surface>.r<R>.p<ramp>.trace (runs whose trace ends are not run again). Per run, on
car_pace.analyse_circle's hold (first latch to first slide):

- `kin_g`: the scorecard's circle g, v²/R over the fastest 1 s on the commanded radius (`latg`);
- `glat_g`: the trace's `glat` (the body-axis specific force: what a body-fixed accelerometer reads, with the roll's
  share of gravity and the sideslip's projection) over the same 1 s, and `glat_p95`: the p95 of |glat| 0.5 s means
  over the hold (rally_gg.py's g-g figure);
- `path_g`: v²/r on the car's own radius from the centre over that 1 s (the kinematic g of the path it drove);
- `beta_deg`: the mean sideslip over that 1 s (`beta`, +left), and the throttle;
- `ratio`: glat_g / kin_g, the factor from the circle row to an accelerometer's reading;
- `hold_s`: the hold's length (s).

`--extra "FLAGS" --tag=T` adds rbr-standalone flags (a run named with T); test tooling such as the environment's
`RERBR_TEST_BRUSH_MU=F` (experimental-brush only, rbr-tyre `car.rs`) passes through to the runs.

target/release/rbr-standalone must be built; RBR_DATA, RBR_RSF and RBR_NGP_PHYSICS come from dev.env.
"""
import argparse
import concurrent.futures
import csv
import math
import os
import statistics
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import car_pace  # noqa: E402
import limit_pace  # noqa: E402  (imports dev_env)

G = car_pace.G
WINDOW = car_pace.STEP_WINDOW  # 1 s of steps
HALF = WINDOW // 2
CARS = "Ford_Fiesta_Rally2_ngp7"


def columns(path):
    """The trace's rows as lists of floats, and its column index by name."""
    with open(path) as f:
        names = f.readline().split()[1:]
        rows = [[float(x) for x in line.split()] for line in f if line.strip() and not line.startswith("#")]
    return rows, {n: i for i, n in enumerate(names)}


def command(car, physics, surface, radius, ramp, trace, extra=()):
    """The rbr-standalone circle run on `surface` (limit_pace.SURFACES) at `radius` m and `ramp` m/s per s."""
    sf = limit_pace.SURFACES[surface]
    cx, cy, _ = sf["centre"]
    steps = min(int(720 * (15 + 25 / float(ramp))), 120000)  # the approach and the ramp to 33 m/s
    return [limit_pace.BIN, "--autopilot", "/dev/null", "--boot", *sf["circle_stage"], "--rsf-car", car,
            "--physics", physics, "--steps", str(steps), "--top-speed", "60", "--circle", f"{cx:.0f},{cy:.0f},{radius}",
            "--circle-ramp", str(ramp), "--trace-speed", trace, *limit_pace.env_args(), *extra]


def own_ramp(surface):
    """The circle ramp (m/s per s) `surface`'s flags give, else the autopilot's 0.5."""
    circle = limit_pace.SURFACES[surface]["circle"]
    return circle[3] if len(circle) > 2 else "0.5"


def analyse(path, cx, cy, radius):
    r = car_pace.analyse_circle(car_pace.load_trace(path), cx, cy, radius)
    if not r["ok"]:
        return {"ok": False, "why": r["reason"]}
    rows, c = columns(path)
    win = rows[r["i_best"]:r["i_best"] + WINDOW]
    glat = statistics.mean(row[c["glat"]] for row in win)
    path_g = statistics.mean(row[c["speed"]] ** 2 / math.hypot(row[c["x"]] - cx, row[c["y"]] - cy) for row in win) / G
    hold = rows[r["i_latch"]:r["i_end"]]
    halves = [abs(statistics.mean(row[c["glat"]] for row in hold[i:i + HALF]))
              for i in range(0, len(hold) - HALF, HALF)]
    p95 = sorted(halves)[min(len(halves) - 1, int(0.95 * len(halves)))] if halves else float("nan")
    return {
        "ok": True, "kin_g": r["latg"], "vmax_kmh": r["vmax"], "glat_g": abs(glat), "glat_p95": p95, "path_g": path_g,
        "beta_deg": math.degrees(statistics.mean(row[c["beta"]] for row in win)),
        "thr": statistics.mean(row[c["thr"]] for row in win), "ratio": abs(glat) / r["latg"],
        "hold_s": (r["i_end"] - r["i_latch"]) / WINDOW,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", required=True)
    parser.add_argument("--cars", default=CARS)
    parser.add_argument("--physics", default="ngp,experimental-brush")
    parser.add_argument("--surfaces", default="gravel,snow,tarmac")
    parser.add_argument("--radii", default="40")
    parser.add_argument("--ramps", default="", help="m/s per s, comma-separated (empty: the surface's own)")
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--extra", default="", help="more rbr-standalone flags (space-separated), after the surface's")
    parser.add_argument("--tag", default="", help="a name part for runs with --extra")
    parser.add_argument("--csv")
    args = parser.parse_args()
    if not os.path.exists(limit_pace.BIN):
        sys.exit(f"{limit_pace.BIN}: build it first (cargo build --release -p rbr-standalone)")
    os.makedirs(args.out, exist_ok=True)
    runs, jobs = [], []
    for car in args.cars.split(","):
        for physics in args.physics.split(","):
            for surface in args.surfaces.split(","):
                cx, cy, _ = limit_pace.SURFACES[surface]["centre"]
                own = own_ramp(surface)
                for radius in args.radii.split(","):
                    for ramp in (args.ramps or own).split(","):
                        name = os.path.join(args.out, f"{car}.{physics}.{surface}{args.tag}.r{radius}.p{ramp}")
                        runs.append((car, physics, surface, float(radius), ramp, name, cx, cy))
                        log = name + ".log"
                        if os.path.exists(log) and "ended by" in open(log).read():
                            continue
                        jobs.append((command(car, physics, surface, radius, ramp, name + ".trace", args.extra.split()),
                                     log))
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        list(pool.map(limit_pace.run, jobs))
    keys = ["kin_g", "glat_g", "ratio", "glat_p95", "path_g", "beta_deg", "thr", "vmax_kmh", "hold_s"]
    out = []
    for car, physics, surface, radius, ramp, name, cx, cy in runs:
        a = analyse(name + ".trace", cx, cy, radius)
        values = [f"{a[k]:.3f}" for k in keys] if a["ok"] else [""] * len(keys)
        row = [car, physics, surface + args.tag, radius, ramp] + values
        out.append(row + ["" if a["ok"] else a["why"]])
        print(" ".join(str(x) for x in row), "" if a["ok"] else a["why"])
    if args.csv:
        with open(args.csv, "w", newline="") as f:
            w = csv.writer(f)
            w.writerow(["car", "physics", "surface", "radius_m", "ramp"] + keys + ["why"])
            w.writerows(out)


if __name__ == "__main__":
    main()
