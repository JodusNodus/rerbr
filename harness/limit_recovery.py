"""limit_recovery.py: recovery past the limit on a flat tarmac circle, per car and physics, through the Rust autopilot.

  python3 -I -B harness/limit_recovery.py --out DIR [--physics ngp,experimental-brush]
      [--cars Ford_Fiesta_Rally2_ngp7,VW_Golf_II_GTI_16V_GrpA_ngp6,Ford_Escort_Mk_II_RS_Grp4_ngp6] [--throttle 0,1]
      [--stage track-85_M] [--circle -260,120,40] [--ramp 0.5] [--steps 30000] [--jobs 6] [--analyse-only]

Each run is `rbr-standalone --autopilot --boot --circle X,Y,R --circle-past T --trace-speed`: the car ramps up on the
circle until it first runs 1 m wide (past the limit), then holds throttle T (0 lifts, 1 holds full throttle) with no
brake or gear change while the pure pursuit keeps steering for the circle (it countersteers a car whose nose points
inside its target, as a driver would). Closed loop rather than an open-loop step steer: the largest flat tarmac on
hand (map 85's r40 circle) has no room for a step steer at speed, and RSF 568's start straight resets the car 2.4 m
off its line (outside assistance), so the manoeuvre has to stay on the circle. The limit comes at 65-70 km/h there.

Per run, from the onset (the first step 1 m wide): the speed and the lateral g at the limit (the 0.25 s mean of
|glat| at its highest in the 3 s before), the lateral g kept (the lowest 0.25 s mean in the 1.5 s after over the
limit's), the widest the car runs (m outside the circle) and when, the time back within 0.5 m of the circle
(recovered; none if never), the peak body sideslip (deg, |beta|), and spun (|beta| over 60 deg at any point after).
Traces and logs go to DIR. target/release/rbr-standalone must be built; RBR_DATA, RBR_RSF and RBR_NGP_PHYSICS come
from dev.env."""
import argparse
import concurrent.futures
import math
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import dev_env  # noqa: E402,F401  (dev.env: RBR_DATA, RBR_RSF, RBR_NGP_PHYSICS)

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(REPO, "target", "release", "rbr-standalone")
G = 9.80665
MEAN = 0.25  # s: the lateral g's moving mean
BEFORE, AFTER = 3.0, 1.5  # s: the limit's window before the onset, the kept grip's after it
BACK = 0.5  # m: back on the circle
SPUN = 60.0  # deg of body sideslip


def command(args, car, physics, throttle, name):
    return [BIN, "--autopilot", os.path.join(args.out, name + ".out"), "--boot", "--stage", args.stage,
            "--rsf-car", car, "--physics", physics, "--steps", str(args.steps), "--top-speed", "60",
            "--circle", args.circle, "--circle-ramp", str(args.ramp), "--circle-past", str(throttle),
            "--trace-speed", os.path.join(args.out, name + ".trace"), "--data", os.environ["RBR_DATA"],
            "--ngp-physics", os.environ["RBR_NGP_PHYSICS"]]


def run(cmd, log, out):
    with open(log, "w") as f:
        code = subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT).returncode
    if os.path.exists(out):
        os.remove(out)  # the playback: 0.1-1 GB, not needed
    return code


def load(path):
    """The trace's rows: time, speed, position, glat, since, yaw, beta."""
    rows = []
    with open(path) as handle:
        for line in handle:
            if line.startswith("#") or not line.strip():
                continue
            f = line.split()
            rows.append(dict(t=float(f[1]), v=float(f[2]), x=float(f[7]), y=float(f[8]), str=float(f[11]),
                             glat=float(f[19]), since=float(f[20]), beta=float(f[22])))
    return rows


def analyse(rows, cx, cy, radius):
    onset = next((i for i, r in enumerate(rows) if r["since"] >= 0.0), None)
    if onset is None:
        return None
    dt = rows[1]["t"] - rows[0]["t"]
    n = max(1, round(MEAN / dt))
    glat = [abs(r["glat"]) for r in rows]
    csum = [0.0]
    for g in glat:
        csum.append(csum[-1] + g)
    mean = lambda i: (csum[i + 1] - csum[max(0, i + 1 - n)]) / min(n, i + 1)  # noqa: E731
    before = range(max(0, onset - round(BEFORE / dt)), onset)
    after = range(onset, min(len(rows), onset + round(AFTER / dt)))
    limit = max(mean(i) for i in before)
    kept = min(mean(i) for i in after) / limit
    wide = [math.hypot(r["x"] - cx, r["y"] - cy) - radius for r in rows]
    tail = range(onset, len(rows))
    widest = max(tail, key=lambda i: wide[i])
    back = next((i for i in range(widest, len(rows)) if wide[i] < BACK), None)
    beta = max(abs(math.degrees(rows[i]["beta"])) for i in tail)
    return dict(v=rows[onset]["v"] * 3.6, limit=limit, kept=kept, widest=wide[widest],
                at=rows[widest]["t"] - rows[onset]["t"],
                back=None if back is None else rows[back]["t"] - rows[onset]["t"], beta=beta, spun=beta > SPUN,
                left=rows[-1]["t"] - rows[onset]["t"])


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--out", required=True)
    parser.add_argument("--physics", default="ngp,experimental-brush")
    parser.add_argument("--cars", default="Ford_Fiesta_Rally2_ngp7,VW_Golf_II_GTI_16V_GrpA_ngp6,"
                                          "Ford_Escort_Mk_II_RS_Grp4_ngp6")
    parser.add_argument("--throttle", default="0,1")
    parser.add_argument("--stage", default="track-85_M")
    parser.add_argument("--circle", default="-260,120,40")
    parser.add_argument("--ramp", type=float, default=0.5)
    parser.add_argument("--steps", type=int, default=30000)
    parser.add_argument("--jobs", type=int, default=6)
    parser.add_argument("--analyse-only", action="store_true")
    args = parser.parse_args()
    os.makedirs(args.out, exist_ok=True)
    runs = [(car, physics, t, f"{car.split('_ngp')[0]}-{physics}-t{t}") for car in args.cars.split(",")
            for physics in args.physics.split(",") for t in args.throttle.split(",")]
    if not args.analyse_only:
        with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
            codes = list(pool.map(lambda r: run(command(args, *r), os.path.join(args.out, r[3] + ".log"),
                                                os.path.join(args.out, r[3] + ".out")), runs))
        for r, code in zip(runs, codes):
            if code:
                print(f"{r[3]}: exit {code} (see its log)")
    cx, cy, radius = (float(x) for x in args.circle.split(","))
    print(f"{'run':58} {'km/h':>5} {'lat g':>6} {'kept':>5} {'wide m':>6} {'at s':>5} {'back s':>6} {'beta':>5} spun")
    for car, physics, t, name in runs:
        path = os.path.join(args.out, name + ".trace")
        a = analyse(load(path), cx, cy, radius) if os.path.exists(path) else None
        if a is None:
            print(f"{name:58} never past the limit")
            continue
        back = "never" if a["back"] is None else f"{a['back']:.2f}"
        print(f"{name:58} {a['v']:5.1f} {a['limit']:6.3f} {a['kept']:5.2f} {a['widest']:6.2f} {a['at']:5.2f} "
              f"{back:>6} {a['beta']:5.1f} {'yes' if a['spun'] else 'no'}")


if __name__ == "__main__":
    main()
