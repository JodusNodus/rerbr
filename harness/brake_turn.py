"""brake_turn.py: braking in a turn on a flat pad under a steering driver, against real rally cars.

  python3 -I -B harness/brake_turn.py run --out DIR [--material 49] [--car NAME] [--physics P,Q] [--jobs N]
  python3 -I -B harness/brake_turn.py read DIR
  python3 -I -B harness/brake_turn.py real TELEMETRY_DIR

`run`: one drive on pad-<material> per physics profile, circle (RADII m), lateral g before the brake (LATERALS) and
pedal (PEDALS): `--circle` ramps the car up its circle and `--brake-from`, at the speed of that lateral g, holds the
pedal to a standstill while the pursuit goes on steering the circle (it countersteers a car whose nose turns in).
Traces to DIR/<physics>.r<R>.a<lateral>.p<pedal>.trace (logs .log).

`read`: per run the body's lateral g before the brake, its deceleration and lateral g over the first 0.5 s on the
pedal (the trace's `glong glat`, as an accelerometer), and above CRAWL km/h the largest sideslip and how far the car
ran outside its circle; per physics the runs past SPUN deg and those more than WIDE m wide. Then `real`'s windows of
the same runs.

`real`: a directory of WRC+ merged telemetry (`rally_gg.py`'s format). The 0.5 s means above MOVING km/h that brake
at HARD g: their lateral g. And `rally_wot.py sideslip`'s 2 s windows (the path's acceleration from the positions,
the sideslip from the accelerometer's direction against it) above MOVING km/h with TURNING g of lateral or more, by
the path's deceleration: the sideslip and the path's whole g. The game's true sideslip is beside its estimate.
"""
import argparse
import concurrent.futures
import glob
import math
import os
import re
import subprocess
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import dev_env  # noqa: E402,F401  (dev.env: RBR_DATA, RBR_NGP_PHYSICS)
import car_pace  # noqa: E402
import corner_limit  # noqa: E402
import rally_wot  # noqa: E402

BIN = corner_limit.BIN
G = corner_limit.G
CENTRE = (car_pace.PAD_CIRC_CX, car_pace.PAD_CIRC_CY)
RADII = (65, 100, 200)
LATERALS = (0.6, 0.7, 0.8, 0.88)
PEDALS = (0.3, 0.45, 0.6, 0.75, 0.9, 1.0)
MOVING, CRAWL = 60.0, 30.0  # km/h
HARD = (0.7, 1.1)  # g
SPUN, WIDE = 45.0, 2.0  # deg, m
BANDS = ((0.3, 0.5), (0.5, 0.7), (0.7, 1.3))  # g of the path's deceleration
TURNS = ((0.3, 0.6), (0.6, 2.0))  # g of the path's lateral


def drive(job):
    name, cmd = job
    if os.path.exists(name + ".log") and "rally:" in open(name + ".log").read():
        return 0
    with open(name + ".log", "w") as f:
        return subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT).returncode


def run(args):
    if not os.path.exists(BIN):
        sys.exit(f"{BIN}: build it first (cargo build --release -p rbr-standalone)")
    os.makedirs(args.out, exist_ok=True)
    jobs = []
    for p in args.physics.split(","):
        for r in RADII:
            for a in LATERALS:
                for pedal in PEDALS:
                    name = os.path.join(args.out, f"{p}.r{r}.a{a:.2f}.p{pedal:.2f}")
                    jobs.append((name, [
                        BIN, "--autopilot", "/dev/null", "--boot", "--stage", f"pad-{args.material}", "--rsf-car",
                        args.car, "--physics", p, "--steps", "70000", "--top-speed", "75", "--circle",
                        f"{CENTRE[0]},{CENTRE[1]},{r}", "--circle-ramp", "0.5", "--brake-from",
                        f"{math.sqrt(a * G * r):.2f}", "--brake-pedal", str(pedal), "--trace-speed", name + ".trace",
                        *corner_limit.env_args()]))
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        codes = list(pool.map(drive, jobs))
    print(f"{len(jobs)} runs, {sum(c != 0 for c in codes)} with a nonzero exit")


def load(path):
    """The run from 4 s before the pedal: columns by name at about 100 Hz, `t` from the pedal, `rho` the distance
    outside the circle; None if the pedal never came."""
    key = re.match(r"(.+)\.r(\d+)\.a([\d.]+)\.p([\d.]+)\.trace", os.path.basename(path))
    radius, pedal = int(key.group(2)), float(key.group(4))
    with open(path) as f:
        names = f.readline().split()[1:]
        cols = ("time", "speed", "brk", "x", "y", "z", "glong", "glat", "beta")
        rows = [[float(v[names.index(n)]) for n in cols] for k, line in enumerate(f) if k % 7 == 0
                for v in [line.split()] if len(v) == len(names)]
    d = dict(zip(cols, np.array(rows).T))
    on = (d["brk"] > 0.5 * pedal) & (d["time"] > 15)
    if not on.any():
        return None
    d["t"] = d["time"] - d["time"][on.argmax()]
    d["rho"] = np.hypot(d["x"] - CENTRE[0], d["y"] - CENTRE[1]) - radius
    keep = d["t"] > -4
    return (key.group(1), radius, float(key.group(3)), pedal), {k: v[keep] for k, v in d.items()}


def windows(rows, lag):
    return [w for w in rally_wot.sideslip_windows(rows, lag) if w[0] > MOVING]


def table(name, win):
    print(f"{name}: by the path's deceleration and lateral g, 2 s windows: n, sideslip p25 / p50 / p75 / p90 deg, "
          "the path's whole g p50 / p90" + (", the true sideslip p50 / p90" if win and win[0][3] is not None else ""))
    for lo, hi in BANDS:
        for la, lb in TURNS:
            w = [x for x in win if lo <= -x[7] < hi and la <= x[1] < lb]
            if len(w) < 12:
                continue
            slip = " / ".join(f"{v:4.1f}" for v in np.percentile([x[2] for x in w], (25, 50, 75, 90)))
            whole = " / ".join(f"{v:.2f}" for v in np.percentile([x[4] for x in w], (50, 90)))
            true = (" | " + " / ".join(f"{v:.0f}" for v in np.percentile([x[3] for x in w], (50, 90)))
                    if w[0][3] is not None else "")
            print(f"  {lo:.1f}-{hi:.1f} g, lateral {la:.1f}-{lb:.1f}: {len(w):5d} | {slip} | {whole}{true}")


def read(args):
    runs, win = {}, {}
    for path in sorted(glob.glob(os.path.join(args.dir, "*.trace"))):
        got = load(path)
        if got is None:
            continue
        key, d = got
        before = (d["t"] >= -1.0) & (d["t"] < -0.2)
        side = np.sign(d["glat"][before].mean())
        first = (d["t"] >= 0.1) & (d["t"] < 0.6)
        live = (d["t"] > 0) & (d["speed"] * 3.6 > CRAWL)
        runs[key] = (d["glat"][before].mean() * side, -d["glong"][first].mean(), d["glat"][first].mean() * side,
                     np.degrees(np.abs(d["beta"][live]).max()), (d["rho"][live] - d["rho"][before].mean()).max())
        s = corner_limit.tenth(dict(d, at=d["time"], off=d["rho"]))
        rows = [dict(t=t, xy=tuple(xy), alt=0.0, long=a, lat=b, true=-c) for t, xy, a, b, c in zip(
            s["t"], s["xy"], s["long"], s["lat"], np.interp(s["t"], d["time"], d["beta"]))]
        win.setdefault(key[0], []).extend(windows(rows, 0.0))
    physics = sorted({k[0] for k in runs})
    for r in RADII:
        for a in LATERALS:
            print(f"\nR {r} m, {a:.2f} g before the brake, per physics ({', '.join(physics)}): the first 0.5 s' "
                  "deceleration / lateral g, the largest sideslip deg, m outside the circle")
            for pedal in PEDALS:
                cells = [runs.get((p, r, a, pedal)) for p in physics]
                print(f"  pedal {pedal:4.2f} | " + " | ".join(
                    f"{x[1]:4.2f} / {x[2]:4.2f} {x[3]:5.1f} {x[4]:5.1f}" if x else f"{'-':>23s}" for x in cells))
    print()
    for p in physics:
        mine = [x for k, x in runs.items() if k[0] == p]
        spun = sum(x[3] > SPUN for x in mine)
        wide = sum(x[3] <= SPUN and x[4] > WIDE for x in mine)
        print(f"{p}: {len(mine)} runs, {spun} past {SPUN:.0f} deg, {wide} more than {WIDE:.0f} m wide")
        table(p, win[p])


def real(args):
    win, lat = [], []
    for path in sorted(glob.glob(os.path.join(args.dir, "df_telemetrymergeddata_*.csv"))):
        rows = rally_wot.load_telemetry(path)
        lag = rally_wot.gps_lag(rows) if len(rows) >= 600 else None
        if lag is None:
            continue
        win += windows(rows, lag)
        along, across = corner_limit.body(*(np.array([r[k] for r in rows]) for k in ("t", "long", "lat")))
        hard = (np.array([r["v"] for r in rows]) > MOVING) & (-along >= HARD[0]) & (-along <= HARD[1])
        lat.append(np.abs(across[hard]))
    lat = np.concatenate(lat)
    print(f"{args.dir}: 0.5 s means above {MOVING:.0f} km/h braking at {HARD[0]}-{HARD[1]} g (n {len(lat)}): lateral g "
          "p50 / p75 / p90 " + " / ".join(f"{v:.2f}" for v in np.percentile(lat, (50, 75, 90)))
          + f", over 0.6 g {100 * (lat > 0.6).mean():.1f} %, over 0.8 g {100 * (lat > 0.8).mean():.1f} %")
    table("real", win)


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--out", required=True)
    r.add_argument("--material", type=int, default=49)
    r.add_argument("--car", default=corner_limit.CAR)
    r.add_argument("--physics", default="experimental,experimental-brush")
    r.add_argument("--jobs", type=int, default=8)
    sub.add_parser("read").add_argument("dir")
    sub.add_parser("real").add_argument("dir")
    args = ap.parse_args()
    {"run": run, "read": read, "real": real}[args.cmd](args)


if __name__ == "__main__":
    main()
