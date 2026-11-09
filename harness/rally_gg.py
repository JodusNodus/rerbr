"""rally_gg.py: whole-car g on a stage, the autopilot's traces against real rally telemetry, filtered alike.

  python3 -I -B harness/rally_gg.py run --out DIR [--rsf-stage 524] [--cars A,B] [--physics ngp,experimental-brush]
      [--steps N] [--jobs N]
  python3 -I -B harness/rally_gg.py stats FILE... [--csv OUT] [--speed-lag S]

`run` drives each car under each physics profile over the RSF stage with the plain autopilot (`rbr-standalone
--autopilot /dev/null --boot --trace-speed`, no playback kept), the traces to DIR/<car>.<physics>.trace (logs .log).
target/release/rbr-standalone must be built; RBR_DATA, RBR_RSF and RBR_NGP_PHYSICS come from dev.env.

`stats` reads each FILE: an autopilot trace (`.trace`: its `glong glat` columns, the specific force on the body's
forward and +X axes in g, `rbr-standalone/src/autopilot/drive.rs`) or a WRC+ merged telemetry CSV
(`df_telemetrymergeddata_*.csv`, RallyDataJunkie; 10 Hz; `speed` km/h, `accx` lateral +left and `accy` longitudinal
+deceleration, both in 0.01 g, `brk` 0-100: units and axes inferred, notes/tyre-model.md). Both become one series at
10 Hz (a trace is averaged over 0.1 s bins; the telemetry is taken as sampled, sorted by time) and pass a 3-sample
median (the telemetry's single-sample spikes); with `--speed-lag S` a trace's speed then passes a first-order lag
of S s, so the game's braking from the speed reads as through a lagging GPS-like channel. Then, on samples over
MOVING km/h:

- braking events: runs of brake over BRAKE_ON (pedal 0.05, telemetry brk 5) lasting at least EVENT_S with at least
  EVENT_DROP km/h lost; their deceleration from the speed (drop over duration, the road's grade included), the
  accelerometer's mean over the event, and its peak 0.5 s mean; the mean over all events and over the LONGEST longest;
- lateral g: |mean| of the lateral channel over consecutive 1 s windows (at least 5 samples, mean speed over MOVING),
  p50 / p90 / p99;
- the g-g envelope per speed bin (BINS km/h): p95 of |lateral|, of deceleration and of drive over 0.5 s means;
- for telemetry, two unit checks over 1 s, above 40 km/h, through zero: the longitudinal channel (-accy/100) against
  the speed's rate of change (near 1 if accy is +deceleration in 0.01 g), and the lateral channel (accx/100) against
  v times the GPS course's rate (near 1 if accx is +left in 0.01 g), with that GPS lateral's p90 (an accelerometer-free
  lateral figure; the course over 1 s smooths it).
"""
import argparse
import bisect
import concurrent.futures
import csv
import math
import os
import statistics
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import dev_env  # noqa: E402,F401  (dev.env: RBR_DATA, RBR_RSF, RBR_NGP_PHYSICS)

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(REPO, "target", "release", "rbr-standalone")
G = 9.80665
RATE = 10.0  # Hz: the telemetry's rate, and the trace's after binning
MOVING = 30.0  # km/h
BRAKE_ON = {"trace": 0.05, "telemetry": 5.0}
EVENT_S = 0.8
EVENT_DROP = 10.0  # km/h
LONGEST = 16
BINS = (30, 60, 90, 120, 150, 999)
CARS = "Hyundai_i20_Coupe_WRC_2020_ngp6,Ford_Fiesta_WRC_2019_ngp6,Ford_Fiesta_Rally2_ngp7"


def run_all(args):
    if not os.path.exists(BIN):
        sys.exit(f"{BIN}: build it first (cargo build --release -p rbr-standalone)")
    os.makedirs(args.out, exist_ok=True)
    jobs = []
    for car in args.cars.split(","):
        for physics in args.physics.split(","):
            name = os.path.join(args.out, f"{car}.{physics}")
            cmd = [BIN, "--autopilot", "/dev/null", "--boot", "--rsf-stage", str(args.rsf_stage), "--rsf-car", car,
                   "--physics", physics, "--steps", str(args.steps), "--trace-speed", name + ".trace",
                   "--data", os.environ["RBR_DATA"], "--ngp-physics", os.environ["RBR_NGP_PHYSICS"]]
            jobs.append((name, cmd))

    def one(job):
        name, cmd = job
        with open(name + ".log", "w") as log:
            return name, subprocess.run(cmd, stdout=log, stderr=subprocess.STDOUT).returncode

    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        for name, code in pool.map(one, jobs):
            ended = [line.strip() for line in open(name + ".log") if "ended by" in line or "race time" in line]
            print(f"{os.path.basename(name)}: exit {code}; {' / '.join(ended)}")


def median3(values):
    return [values[0]] + [sorted(values[i - 1:i + 2])[1] for i in range(1, len(values) - 1)] + values[-1:] \
        if len(values) > 2 else list(values)


def load_trace(path):
    """(t s, speed km/h, brake 0-1, long g, lat g) at 10 Hz: the trace's steps averaged over 0.1 s bins."""
    bins = {}
    with open(path) as handle:
        header = handle.readline().split()[1:]
        col = {name: i for i, name in enumerate(header)}
        if "glong" not in col:
            sys.exit(f"{path}: no glong/glat columns (an older rbr-standalone)")
        for line in handle:
            f = line.split()
            if len(f) < len(header):
                continue
            t = float(f[col["time"]])
            key = int(t * RATE)
            acc = bins.setdefault(key, [0, 0.0, 0.0, 0.0, 0.0])
            acc[0] += 1
            acc[1] += float(f[col["speed"]]) * 3.6
            acc[2] += float(f[col["brk"]])
            acc[3] += float(f[col["glong"]])
            acc[4] += float(f[col["glat"]])
    keys = sorted(bins)[1:]  # the first bin holds the trace's first line (0 g)
    return [((k + 0.5) / RATE, *(v / bins[k][0] for v in bins[k][1:])) for k in keys]


def load_telemetry(path, gps=False):
    """(t s, speed km/h, brk 0-100, long g, lat g) as sampled: long = -accy/100 (accy +deceleration), lat =
    accx/100 (+left); with `gps`, each row ends in its (lon, lat) degrees."""
    rows = []
    with open(path, newline="") as handle:
        for r in csv.DictReader(handle):
            try:
                utx = float(r["utx"])
                rows.append((utx / 1000.0, float(r["speed"]), float(r["brk"] or 0), -float(r["accy"]) / 100.0,
                             float(r["accx"]) / 100.0, *((float(r["lon"]), float(r["lat"])) if gps else ())))
            except (ValueError, KeyError):
                continue
    rows = sorted(r for r in rows if r[0] > 0)
    t0 = rows[0][0]
    return [(r[0] - t0, *r[1:]) for r in rows]


def filtered(rows):
    cols = list(zip(*rows))
    return list(zip(cols[0], cols[1], cols[2], median3(list(cols[3])), median3(list(cols[4]))))


def windows(rows, width):
    """Means over consecutive windows of `width` s with at least half their samples: (speed, brk, long, lat)."""
    groups = {}
    for r in rows:
        groups.setdefault(int(r[0] // width), []).append(r)
    need = max(1, int(width * RATE / 2))
    return [tuple(statistics.fmean(r[i] for r in g) for i in (1, 2, 3, 4)) for g in groups.values() if len(g) >= need]


def pct(values, p):
    if not values:
        return float("nan")
    s = sorted(values)
    return s[min(len(s) - 1, int(round(p / 100 * (len(s) - 1))))]


def braking_events(rows, on):
    events, i = [], 0
    while i < len(rows):
        if rows[i][2] > on and rows[i][1] > MOVING:
            j = i
            while j + 1 < len(rows) and rows[j + 1][2] > on and rows[j + 1][0] - rows[j][0] < 0.5:
                j += 1
            dur = rows[j][0] - rows[i][0]
            drop = rows[i][1] - rows[j][1]
            if dur >= EVENT_S and drop >= EVENT_DROP:
                body = rows[i:j + 1]
                decel = [-r[3] for r in body]
                peak = max(statistics.fmean(decel[k:k + 5]) for k in range(max(1, len(decel) - 4)))
                events.append({"dur": dur, "from": rows[i][1], "drop": drop, "speed_g": drop / 3.6 / dur / G,
                               "acc_g": statistics.fmean(decel), "peak_g": peak})
            i = j + 1
        else:
            i += 1
    return events


def slope(pairs):
    """Least squares through 0 of y on x."""
    sxx = sum(x * x for x, _ in pairs)
    return sum(x * y for x, y in pairs) / sxx if sxx else float("nan")


def unit_checks(rows):
    """Over 1 s steps above 40 km/h, the raw rows with GPS (`load_telemetry(gps=True)`): (long channel against the
    speed's rate, lateral channel against v times the GPS course's rate, that GPS lateral's |p90|), all g."""
    ts = [r[0] for r in rows]

    def at(t):
        return rows[min(bisect.bisect_left(ts, t), len(rows) - 1)]

    def mean(t, i):
        body = rows[bisect.bisect_left(ts, t - 0.5):bisect.bisect_left(ts, t + 0.5)]
        return statistics.fmean(r[i] for r in body) if body else None

    def course(p, q):
        dx = (q[5] - p[5]) * math.cos(math.radians(p[6])) * 111320.0
        dy = (q[6] - p[6]) * 110540.0
        return math.atan2(dy, dx), math.hypot(dx, dy)

    long_pairs, lat_pairs = [], []
    t = ts[0] + 2.0
    while t < ts[-1] - 2.0:
        a, b, c = at(t - 1.0), at(t), at(t + 1.0)
        acc, lat = mean(t, 3), mean(t, 4)
        if b[1] > 40.0 and acc is not None:
            long_pairs.append(((c[1] - a[1]) / 3.6 / 2.0 / G, acc))
            (h1, d1), (h2, d2) = course(a, b), course(b, c)
            if d1 > 5.0 and d2 > 5.0:
                turn = (h2 - h1 + math.pi) % (2 * math.pi) - math.pi
                lat_pairs.append((b[1] / 3.6 * turn / G, lat))
        t += 1.0
    return slope(long_pairs), slope(lat_pairs), pct([abs(x) for x, _ in lat_pairs], 90)


def lagged(rows, tau):
    """The rows with the speed through a first-order lag of `tau` s (at the 10 Hz rows), as a GPS-like speed channel
    trails the car: the telemetry's speed is GPS-like (notes/tyre-model.md "Car-level grip in the same terms")."""
    if tau <= 0:
        return rows
    k = 1.0 - math.exp(-1.0 / RATE / tau)
    out, v = [], rows[0][1]
    for r in rows:
        v += k * (r[1] - v)
        out.append((r[0], v, *r[2:]))
    return out


def stats(path, speed_lag=0.0):
    kind = "trace" if path.endswith(".trace") else "telemetry"
    out = {"file": os.path.basename(path), "kind": kind}
    out.update(summary(load_trace(path) if kind == "trace" else load_telemetry(path), kind, speed_lag))
    checks = unit_checks(load_telemetry(path, gps=True)) if kind == "telemetry" else (float("nan"),) * 3
    out.update(zip(("long_slope", "lat_slope", "gps_lat_p90"), checks))
    return out


def summary(rows, kind, speed_lag=0.0):
    """The figures of `stats` over 10 Hz rows (t s, speed km/h, brake, long g, lat g) of `kind` (`trace` or
    `telemetry`: the brake's scale); a trace's speed through `speed_lag` (`lagged`)."""
    rows = filtered(rows)
    if kind == "trace":
        rows = lagged(rows, speed_lag)
    moving = [r for r in rows if r[1] > MOVING]
    events = braking_events(rows, BRAKE_ON[kind])
    longest = sorted(events, key=lambda e: e["dur"], reverse=True)[:LONGEST]
    lat1 = [abs(w[3]) for w in windows(moving, 1.0) if w[0] > MOVING]
    half = [w for w in windows(moving, 0.5) if w[0] > MOVING]
    out = {"moving_s": len(moving) / RATE,
           "vmean": statistics.fmean(r[1] for r in moving), "vmax": max(r[1] for r in moving),
           "events": len(events),
           "brk_speed_all": statistics.fmean(e["speed_g"] for e in events) if events else float("nan"),
           "brk_speed_16": statistics.fmean(e["speed_g"] for e in longest) if longest else float("nan"),
           "brk_acc_16": statistics.fmean(e["acc_g"] for e in longest) if longest else float("nan"),
           "brk_peak_16": statistics.median(e["peak_g"] for e in longest) if longest else float("nan"),
           "lat_p50": pct(lat1, 50), "lat_p90": pct(lat1, 90), "lat_p99": pct(lat1, 99),
           }
    for lo, hi in zip(BINS, BINS[1:]):
        b = [w for w in half if lo <= w[0] < hi]
        out[f"gg{lo}"] = (len(b), pct([abs(w[3]) for w in b], 95), pct([-w[2] for w in b if w[2] < 0], 95),
                          pct([w[2] for w in b if w[2] > 0], 95))
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="mode", required=True)
    r = sub.add_parser("run")
    r.add_argument("--out", required=True)
    r.add_argument("--rsf-stage", type=int, default=524)
    r.add_argument("--cars", default=CARS)
    r.add_argument("--physics", default="ngp,experimental-brush")
    r.add_argument("--steps", type=int, default=500000)
    r.add_argument("--jobs", type=int, default=6)
    s = sub.add_parser("stats")
    s.add_argument("files", nargs="+")
    s.add_argument("--csv")
    s.add_argument("--speed-lag", type=float, default=0.0,
                   help="s: a trace's speed through a first-order lag first (the telemetry's GPS-like speed)")
    args = parser.parse_args()
    if args.mode == "run":
        return run_all(args)
    results = [stats(f, args.speed_lag) for f in args.files]
    print("file | kind | moving s | v mean/max km/h | brake events | decel speed all/16 longest | acc mean 16 | "
          "peak 0.5 s med 16 | lat 1 s p50/p90/p99 | long/lat channel slope | GPS lat p90")
    for o in results:
        print(f"{o['file']} | {o['kind']} | {o['moving_s']:.0f} | {o['vmean']:.0f}/{o['vmax']:.0f} | {o['events']} | "
              f"{o['brk_speed_all']:.2f}/{o['brk_speed_16']:.2f} | {o['brk_acc_16']:.2f} | {o['brk_peak_16']:.2f} | "
              f"{o['lat_p50']:.2f}/{o['lat_p90']:.2f}/{o['lat_p99']:.2f} | {o['long_slope']:.2f}/{o['lat_slope']:.2f} | "
              f"{o['gps_lat_p90']:.2f}")
    print("g-g p95 over 0.5 s means per speed bin (n, |lat|, decel, drive):")
    for o in results:
        cells = [f"{lo}-{hi}: {o[f'gg{lo}'][0]} {o[f'gg{lo}'][1]:.2f}/{o[f'gg{lo}'][2]:.2f}/{o[f'gg{lo}'][3]:.2f}"
                 for lo, hi in zip(BINS, BINS[1:]) if o[f"gg{lo}"][0]]
        print(f"{o['file']}: " + "; ".join(cells))
    if args.csv:
        keys = [k for k in results[0] if not k.startswith("gg")]
        with open(args.csv, "w", newline="") as handle:
            w = csv.writer(handle)
            w.writerow(keys + [f"gg{lo}_{q}" for lo in BINS[:-1] for q in ("n", "lat", "decel", "drive")])
            for o in results:
                w.writerow([o[k] for k in keys] + [x for lo in BINS[:-1] for x in o[f"gg{lo}"]])


if __name__ == "__main__":
    main()
