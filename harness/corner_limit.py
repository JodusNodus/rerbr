"""corner_limit.py: the lateral g and the speed a car holds in each corner of a stage, against real rally cars on
the same road.

  python3 -I -B harness/corner_limit.py run --out DIR [--rsf-stage 524] [--car NAME] [--physics P,Q]
      [--levels 0.7,0.75,...] [--top-speed 50] [--race LAT,BRAKE,SLIP] [--jobs N]
  python3 -I -B harness/corner_limit.py read DIR [--real TELEMETRY_DIR --stage SS7 [--ref DRIVER]] [--real-floor G]
  python3 -I -B harness/corner_limit.py demand DIR... --real TELEMETRY_DIR [--stage SS7] [--ref DRIVER]
  python3 -I -B harness/corner_limit.py circle X,Y TRACE...

`run` drives the stage once with the plain autopilot per physics profile (DIR/base.<physics>.trace, the first one's
path gives the corners: DIR/corners.json), then once per level and corner group with `--speed-zone` on each corner
of the group: the car arrives at one speed, sqrt(level g x the corner's radius), LEAD m before the corner and holds
it to TAIL m after, the rest of the stage at the default plan. A group's zones lie GAP m apart, so a corner lost
does not reach the next one tested. Traces to DIR/<physics>.a<level>.g<group>.trace (logs .log). With `--race` every
run drives the race autopilot's line (the least bend inside the road's edges, `autopilot::race`; 5,4,0.3 is the
plain plan's level under the default's steering) and its path gives the corners; a car that loses that line does
not recover, so each corner has its own run, ended PAST m after it.

A corner is a stretch of the base run's path that turns one way: around a peak of the path's curvature (over
+-SPAN m) of 1/R_MAX or more, out to where it falls under SHARE of the peak, turning TURN rad or more, its radius
(1 over the peak) R_MIN m or more.

`read`: per corner, the lateral g towards it, two ways, both at 10 Hz. The body's: the peak of the 0.5 s mean of
the specific force across the car (the trace's `glat`; a real car's accelerometer) while the force along it stays
under COAST g. At the limit it is the tyres' share of the car's weight nearly whatever the road's banking, which
the path's g includes. The path's: the peak of the lateral acceleration of a second-order fit to 1 s of positions.
The game's figures are those of the fastest pass that held the corner where a faster one did not: a pass holds it
within WIDE m of the line (the driveline, or the base run's path under `--race`) and under SLID rad of sideslip. That
is the limit from below, a level's step under the first pass that lost it. With `--real`, the same reading of each
real drive (WRC+ merged telemetry, `rally_gg.py`'s format) laid on the game's path: the reference driver's GPS path
and the base run's are matched by their curvature and a rigid fit, and each drive's samples take the driveline
distance of the nearest point of the game's path. The summary takes the corners whose real median is `--real-floor`
g or more (the crews near their limit), and sets the game's speed against the real drives' slowest in the corner.
What it cannot show: a corner's own banking, crest or dip where the stage and the road differ, and what a driver
adds who turns the car before the corner; the spread from corner to corner is +-25 %.

`demand`: whether a line is the real drives' line. Per corner and real drive, each DIR's base path is driven at
that drive's own speed by driveline distance and read by the same 1 s fit: the lateral g the line asks at the real
speeds, over the g the drive pulled. 1 is the real line; the driveline asks more wherever the crews straighten it.
Corners and the fit from the first DIR.

`circle`: the same two figures of a `--circle X,Y,R` ramp (TRACE named `*.rR.trace`), while the car is within
OFF_CIRCLE m of the circle: the car's limit on a flat pad, read as the corners are.
"""
import argparse
import concurrent.futures
import glob
import json
import math
import os
import re
import subprocess
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import dev_env  # noqa: E402,F401  (dev.env: RBR_DATA, RBR_RSF, RBR_NGP_PHYSICS)
import rally_wot  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(REPO, "target", "release", "rbr-standalone")
G = 9.80665
CAR = "Hyundai_i20_Coupe_WRC_2020_ngp6"
LEVELS = "0.7,0.75,0.8,0.85,0.9,0.95,1.0,1.05,1.1,1.15,1.2,1.3,1.4,1.5"
STEPS = 432000
DS = 2.0  # m: the path's grid
SPAN = 6.0  # m: the heading's and the curvature's differences reach this far either side
R_MIN, R_MAX = 20.0, 250.0
SHARE = 0.4
TURN = 0.15
LEAD, TAIL, GAP = 40.0, 10.0, 150.0
HOLD = 30.0  # m past the corner still read for running wide
PAST = 200.0  # m past its corner a `--race` run ends
COAST = 0.3  # g along the car, under which the body's lateral counts as cornering alone
SPUN, CRAWL = 0.79, 8.0  # rad, m/s: past either the car is not cornering
WIDE, SLID = 1.5, 0.26  # m, rad: a pass past the corner's limit
SPEEDS = ((0, 90), (90, 120), (120, 400))  # km/h: the real corners' bands
LONG = 0.52  # rad: a corner turning this much holds the car at its limit for the readings' windows
REAL_PAD = 25.0  # m: a real drive's corner window is this much wider (the paths' fit)
OFF_CIRCLE = 6.0  # m


def trace(path, stride=7):
    """Every `stride`-th row of a `--trace-speed` file, as columns by name, with `at`: the driveline distance, from
    the log's lines (every 500 steps) by step."""
    with open(path) as f:
        names = f.readline().split()[1:]
        cols = ("step", "time", "speed", "x", "y", "off", "beta", "glong", "glat")
        want = [names.index(n) for n in cols]
        rows = [[float(v[i]) for i in want] for k, line in enumerate(f) if k % stride == 0
                for v in [line.split()] if len(v) == len(names)]
    a = np.array(rows)
    d = dict(zip(cols, a.T))
    log = open(path[:-len(".trace")] + ".log").read()
    at = np.array([(int(s), float(m)) for s, m in re.findall(r"autopilot: step (\d+): at ([0-9.]+) of", log)])
    d["at"] = np.interp(d["step"], at[:, 0], at[:, 1])
    return d


def lateral(t, xy, half=0.5):
    """The path's speed (m/s) and lateral acceleration (m/s², + to the left of travel) at each sample: a second-order
    fit to the positions within `half` s; nan with under 8 samples or 1.6 `half` of span."""
    n = len(t)
    v, lat = np.full(n, np.nan), np.full(n, np.nan)
    lo = hi = 0
    for i in range(n):
        while t[lo] < t[i] - half - 1e-6:
            lo += 1
        while hi + 1 < n and t[hi + 1] <= t[i] + half + 1e-6:
            hi += 1
        if hi - lo + 1 < 8 or t[hi] - t[lo] < 1.6 * half:
            continue
        dt = t[lo:hi + 1] - t[i]
        cx, cy = np.polyfit(dt, xy[lo:hi + 1, 0], 2), np.polyfit(dt, xy[lo:hi + 1, 1], 2)
        vx, vy, ax, ay = cx[1], cy[1], 2 * cx[0], 2 * cy[0]
        v[i] = math.hypot(vx, vy)
        if v[i] > 1.0:
            lat[i] = (vx * ay - vy * ax) / v[i]
    return v, lat


def grid_path(d, ds=DS):
    """The run's path on a `ds` grid of driveline distance: distances and positions."""
    at = np.maximum.accumulate(d["at"])
    keep = np.concatenate([[True], np.diff(at) > 1e-3])
    s = np.arange(at[keep][0], at[keep][-1], ds)
    return s, np.column_stack([np.interp(s, at[keep], d["x"][keep]), np.interp(s, at[keep], d["y"][keep])])


def curvature(p, ds=DS, span=SPAN):
    """The path's curvature (1/m, + to the left) at each grid point (0 at the ends)."""
    k = int(round(span / ds))
    step = p[k:] - p[:-k]
    th = np.unwrap(np.arctan2(step[:, 1], step[:, 0]))
    out = np.zeros(len(p))
    out[k:len(p) - k] = (th[k:] - th[:-k]) / (k * ds)
    return out


def corners(d, alone=False):
    """The corners of the run's path, each with its group: the runs' zones lie GAP m apart, or each corner has its
    own run (`alone`)."""
    s, p = grid_path(d)
    kap = curvature(p)
    mag = np.abs(kap)
    w = int(30 / DS)
    found = []
    for i in range(w, len(s) - w):
        if mag[i] < 1 / R_MAX or mag[i] < mag[i - w:i + w + 1].max():
            continue
        sign = np.sign(kap[i])
        a = b = i
        while a > 0 and kap[a - 1] * sign > SHARE * mag[i]:
            a -= 1
        while b + 1 < len(s) and kap[b + 1] * sign > SHARE * mag[i]:
            b += 1
        turn = abs(kap[a:b + 1].sum() * DS)
        if turn < TURN or 1 / mag[i] < R_MIN or (found and a <= found[-1]["i1"]):
            continue
        found.append(dict(at=float(s[i]), s0=float(s[a]), s1=float(s[b]), radius=float(1 / mag[i]),
                          left=bool(sign > 0), turn=float(turn), i1=b))
    ends = []
    for k, c in enumerate(found):
        del c["i1"]
        g = k if alone else next((g for g, end in enumerate(ends) if c["s0"] - LEAD >= end + GAP), len(ends))
        ends[g:g + 1] = [c["s1"] + TAIL]
        c["group"] = g
    return found


def zone_speed(c, level, top):
    return min(top, math.sqrt(level * G * c["radius"]))


def env_args():
    return ["--data", os.environ["RBR_DATA"], "--ngp-physics", os.environ["RBR_NGP_PHYSICS"]]


def drive(job):
    name, extra = job
    if os.path.exists(name + ".log") and "rally:" in open(name + ".log").read():
        return 0
    cmd = [BIN, "--autopilot", "/dev/null", "--boot", *extra, "--trace-speed", name + ".trace", *env_args()]
    with open(name + ".log", "w") as f:
        return subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT).returncode


def run(args):
    if not os.path.exists(BIN):
        sys.exit(f"{BIN}: build it first (cargo build --release -p rbr-standalone)")
    os.makedirs(args.out, exist_ok=True)
    physics = args.physics.split(",")
    levels = [float(x) for x in args.levels.split(",")]
    common = ["--rsf-stage", str(args.rsf_stage), "--rsf-car", args.car]
    if args.race:
        common += ["--race", args.race]
    base = [(os.path.join(args.out, f"base.{p}"), [*common, "--steps", str(STEPS), "--physics", p]) for p in physics]
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        list(pool.map(drive, base))
        first = trace(base[0][0] + ".trace")
        found = corners(first, alone=bool(args.race))
        with open(os.path.join(args.out, "corners.json"), "w") as f:
            json.dump(dict(top=args.top_speed, levels=levels, race=args.race, base=os.path.basename(base[0][0]),
                           corners=found), f, indent=1)
        groups = 1 + max(c["group"] for c in found)
        print(f"{len(found)} corners in {groups} groups; {len(levels) * groups * len(physics)} runs")
        jobs = []
        for p in physics:
            for level in levels:
                for g in range(groups):
                    zones = [f"{c['s0'] - LEAD:.1f},{c['s1'] + TAIL:.1f},{zone_speed(c, level, args.top_speed):.2f}"
                             for c in found if c["group"] == g]
                    # A corner alone: the run ends PAST m after it, by the base run's steps and a quarter more.
                    end = max(c["s1"] for c in found if c["group"] == g) + PAST
                    steps = int(1.25 * np.interp(end, first["at"], first["step"])) if args.race else STEPS
                    extra = [*common, "--steps", str(steps), "--physics", p, "--top-speed", str(args.top_speed)]
                    for z in zones:
                        extra += ["--speed-zone", z]
                    jobs.append((os.path.join(args.out, f"{p}.a{level:.2f}.g{g}"), extra))
        codes = list(pool.map(drive, jobs))
    print(f"{sum(c != 0 for c in codes)} runs with a nonzero exit")


def tenth(d):
    """The trace at 10 Hz: per 0.1 s the first row's time, position, driveline distance and speed, and the bin's mean
    specific force (g, forward and left) and largest sideslip and distance from the driveline."""
    edge = np.flatnonzero(np.diff(np.floor(d["time"] * 10), prepend=-1) > 0)
    mean = lambda k: np.add.reduceat(d[k], edge) / np.diff(np.append(edge, len(d[k])))
    return dict(t=d["time"][edge], xy=np.column_stack([d["x"][edge], d["y"][edge]]), at=d["at"][edge],
                v=d["speed"][edge], long=mean("glong"), lat=mean("glat"),
                beta=np.maximum.reduceat(np.abs(d["beta"]), edge), off=np.maximum.reduceat(d["off"], edge))


def median3(x):
    return np.median(np.column_stack([np.roll(x, 1), x, np.roll(x, -1)]), axis=1)


def body(t, long, lat):
    """The 0.5 s means (5 samples within 0.7 s, after a 3-sample median) of the body's lateral and longitudinal
    specific force at each sample; nan where the samples do not run on."""
    long, lat = median3(long), median3(lat)
    k = np.ones(5) / 5
    good = np.zeros(len(t), bool)
    good[2:-2] = t[4:] - t[:-4] < 0.7
    return (np.where(good, np.convolve(long, k, "same"), np.nan), np.where(good, np.convolve(lat, k, "same"), np.nan))


def corner_g(found, t, xy, at, long, lat, pad, live=None):
    """Per corner, None or: the path's lateral g towards it (peak of the 1 s fit), the body's (peak of the 0.5 s
    mean with under COAST g along the car; nan if none), the speed at that peak and the slowest speed, km/h. `live`
    masks the samples that count. The body's left is taken from the drive itself: the sign that lines its lateral up
    with the path's."""
    v, path = lateral(t, xy)
    blong, blat = body(t, long, lat)
    both = ~np.isnan(path) & ~np.isnan(blat)
    left = np.sign(np.sum(path[both] * blat[both])) if both.any() else 1.0
    out = []
    for c in found:
        arc = (at >= c["s0"] - pad) & (at <= c["s1"] + pad) & ~np.isnan(path)
        if live is not None:
            arc &= live
        if arc.sum() < 5:
            out.append(None)
            continue
        side = 1 if c["left"] else -1
        coast = arc & (np.abs(blong) < COAST) & ~np.isnan(blat)
        i = np.flatnonzero(coast)[(blat[coast] * side * left).argmax()] if coast.any() else None
        out.append((float((path[arc] * side).max() / G), float(blat[i] * side * left) if i is not None else math.nan,
                    float(v[i] * 3.6) if i is not None else math.nan, float(v[arc].min() * 3.6)))
    return out


def away(line, at, xy, reach=20.0):
    """Each position's distance (m) from the path `line` (`grid_path`), searched within `reach` m of the position's
    own driveline distance."""
    s, p = line
    lo, hi = np.searchsorted(s, at - reach), np.searchsorted(s, at + reach)
    return np.array([np.hypot(*(p[a:b] - q).T).min() if b > a else np.nan for a, b, q in zip(lo, hi, xy)])


def passes(d, found, line=None):
    """Per corner: None, or the pass's path g, body g and speed (`corner_g`, over the samples under SPUN rad of
    sideslip, above CRAWL m/s and within WIDE m of the line), then how far it ran from the driveline (m; from `line`,
    a `--race` run's) and its largest sideslip (rad), and its median speed in the corner (km/h)."""
    s = tenth(d)
    if line is not None:
        s["off"] = away(line, s["at"], s["xy"])
    live = (s["beta"] < SPUN) & (s["v"] > CRAWL) & ~(s["off"] > WIDE)
    g = corner_g(found, s["t"], s["xy"], s["at"], s["long"], s["lat"], 0.0, live)
    out = []
    for c, x in zip(found, g):
        inside = (s["at"] >= c["s0"] - 5) & (s["at"] <= c["s1"] + HOLD)
        arc = (s["at"] >= c["s0"]) & (s["at"] <= c["s1"])
        # A pass with no reading (off the line or spun from the corner's start) still counts as one that lost it.
        out.append(None if not arc.any() else (
            *(x[:3] if x else (math.nan,) * 3), float(np.nanmax(s["off"][inside])), float(s["beta"][inside].max()),
            float(np.median(s["v"][arc]) * 3.6)))
    return out


def similarity(a, b):
    """The scale, rotation (a mirror if it fits better) and shift taking points a onto b."""
    ma, mb = a.mean(0), b.mean(0)
    u, s, vt = np.linalg.svd((b - mb).T @ (a - ma) / len(a))
    best = None
    for flip in (1.0, -1.0):
        dd = np.diag([1.0, flip * np.sign(np.linalg.det(u @ vt))])
        r = u @ dd @ vt
        scale = (s * np.diag(dd)).sum() / ((a - ma) ** 2).sum() * len(a)
        err = np.hypot(*(scale * a @ r.T + (mb - scale * r @ ma) - b).T).mean()
        if best is None or err < best[0]:
            best = (err, scale, r, mb - scale * r @ ma)
    return best[1:]


def nearest(ref, pts, window=60):
    """For each point the nearest reference index, searched on from the last one, and the distance."""
    idx, dist = np.zeros(len(pts), int), np.zeros(len(pts))
    j = int(np.hypot(*(ref - pts[0]).T).argmin())
    for i, p in enumerate(pts):
        lo, hi = max(0, j - 10), min(len(ref), j + window)
        d = np.hypot(*(ref[lo:hi] - p).T)
        j = lo + int(d.argmin())
        idx[i], dist[i] = j, d[j - lo]
    return idx, dist


def resample(xy, ds):
    s = np.concatenate([[0], np.cumsum(np.hypot(*np.diff(xy, axis=0).T))])
    keep = np.concatenate([[True], np.diff(s) > 1e-6])
    grid = np.arange(0, s[keep][-1], ds)
    return np.column_stack([np.interp(grid, s[keep], xy[keep, 0]), np.interp(grid, s[keep], xy[keep, 1])])


def real_drives(directory, stage):
    """Per driver: time (s), position (m, east and north of the first drive's mean) and the accelerometer's
    specific force along the car and across it (g)."""
    out, origin = {}, None
    for path in sorted(glob.glob(os.path.join(directory, f"df_telemetrymergeddata_{stage}_*.csv"))):
        rows = rally_wot.load_telemetry(path)
        t = np.array([r["t"] for r in rows])
        lon, lat = np.array([r["pos"][0] for r in rows]), np.array([r["pos"][1] for r in rows])
        keep = np.concatenate([[True], np.diff(t) > 0])
        origin = origin or (lon.mean(), lat.mean())
        east = np.radians(lon - origin[0]) * math.cos(math.radians(origin[1])) * 6371000.0
        north = np.radians(lat - origin[1]) * 6371000.0
        force = np.array([(r["long"], r["lat"]) for r in rows])[keep]
        out[os.path.basename(path).split("_")[-1][:-4]] = (t[keep] - t[0], np.column_stack([east, north])[keep],
                                                           force[:, 0], force[:, 1])
    return out


def lay(game, ref):
    """The fit taking real positions (m) into the game's frame: the game's path (5 m grid) is found on the reference
    drive's by the curvature profiles' best offset, either handedness, then fitted rigidly with scale and refitted on
    its nearest points. Returns a function of positions and the fit's median and p90 gap (m)."""
    ds = 5.0
    gp, rp = resample(game, ds), resample(ref, ds)
    kg, kr = curvature(gp, ds, 30.0), curvature(rp, ds, 30.0)
    best = None
    for sign in (1.0, -1.0):
        c = np.correlate(kr, sign * kg, mode="valid")
        if best is None or c.max() > best[0]:
            best = (c.max(), int(c.argmax()))
    off = best[1]
    scale, r, shift = similarity(gp, rp[off:off + len(gp)])
    for _ in range(8):
        idx, dist = nearest(rp, scale * gp @ r.T + shift)
        good = dist < max(10.0, np.percentile(dist, 80))
        scale, r, shift = similarity(gp[good], rp[idx[good]])
    _, dist = nearest(rp, scale * gp @ r.T + shift)
    return (lambda xy: (xy - shift) @ r / scale), scale, float(np.median(dist)), float(np.percentile(dist, 90))


def laid(base, directory, stage, ref):
    """Each real drive laid on the game's path: its time, positions in the game's frame, their driveline distances
    (nan over 30 m from the path) and the accelerometer's two axes."""
    drives = real_drives(directory, stage)
    if not drives:
        sys.exit(f"{directory}: no df_telemetrymergeddata_{stage}_*.csv")
    s, path = grid_path(base)
    to_game, scale, gap, gap90 = lay(path, drives[ref if ref in drives else sorted(drives)[0]][1])
    print(f"real drives laid on the game's path: scale {scale:.4f}, gap median {gap:.1f} m, p90 {gap90:.1f} m; "
          f"{len(drives)} drives: {', '.join(sorted(drives))}")
    out = []
    for name, (t, xy, long, lat) in sorted(drives.items()):
        g = to_game(xy)
        near = np.array([np.hypot(*(path - p).T).argmin() for p in g])
        out.append((t, g, np.where(np.hypot(*(path[near] - g).T) < 30.0, s[near], np.nan), long, lat))
    return out


def real_corners(found, base, directory, stage, ref):
    """Per corner: each real drive's path g, body g and speed (`corner_g`)."""
    out = [[] for _ in found]
    for t, g, at, long, lat in laid(base, directory, stage, ref):
        for k, x in enumerate(corner_g(found, t, g, at, long, lat, REAL_PAD)):
            if x is not None:
                out[k].append(x)
    return out


def peak(c, at, lat, pad=REAL_PAD):
    """The largest of `lat` towards corner `c` within `pad` m of it (nan with under 5 samples), and its index."""
    arc = np.flatnonzero((at >= c["s0"] - pad) & (at <= c["s1"] + pad) & ~np.isnan(lat))
    if len(arc) < 5:
        return math.nan, None
    i = arc[(lat[arc] * (1 if c["left"] else -1)).argmax()]
    return float(lat[i] * (1 if c["left"] else -1)), i


def quartiles(v):
    return " / ".join(f"{x:.2f}" for x in np.nanpercentile(v, (25, 50, 75)))


def spec_of(directory):
    """A run's corners.json and the trace of the base run its corners come from."""
    spec = json.load(open(os.path.join(directory, "corners.json")))
    first = sorted(glob.glob(os.path.join(directory, "base.*.trace")))[0]
    return spec, trace(os.path.join(directory, spec["base"] + ".trace") if "base" in spec else first)


def read(args):
    spec, base = spec_of(args.dir)
    found = spec["corners"]
    line = grid_path(base, 0.25) if spec.get("race") else None
    physics = sorted({os.path.basename(p).split(".a")[0] for p in glob.glob(os.path.join(args.dir, "*.a*.g*.trace"))})
    game = {}
    for p in physics:
        got = [[] for _ in found]
        for name in sorted(glob.glob(os.path.join(args.dir, f"{p}.a*.g*.trace"))):
            group = int(name.rsplit(".g", 1)[1].split(".")[0])
            for k, x in enumerate(passes(trace(name), found, line)):
                if x is not None and found[k]["group"] == group:
                    got[k].append(x)
        game[p] = got
    real = real_corners(found, base, args.real, args.stage, args.ref) if args.real else None
    print(f"\nper corner, lateral g towards it: the body's (0.5 s mean under {COAST} g along the car) / the path's "
          "(1 s fit), and km/h.\nreal: the drives' median (highest), km/h at the body's and the slowest in the "
          f"corner. game: the fastest pass that held (within {WIDE} m of the line, under {math.degrees(SLID):.0f} deg "
          "of sideslip) where a faster one did not, [passes]; 'held all': no pass lost it; 'none held'")
    head = "    at   R m dir turn | "
    if real:
        head += "real  n  body med (max)   path med (max)  km/h  slowest | "
    print(head + " | ".join(f"{p:>32s}" for p in physics))
    rows = []
    for k, c in enumerate(found):
        text = f"{c['at']:7.0f} {c['radius']:5.0f}  {'L' if c['left'] else 'R'} {math.degrees(c['turn']):5.0f} | "
        rl = None
        if real:
            r = np.array(real[k]) if real[k] else np.zeros((0, 4))
            ok = r[~np.isnan(r[:, 1])] if len(r) else r
            if len(ok) >= 3:
                rl = dict(body=float(np.median(ok[:, 1])), path=float(np.median(r[:, 0])),
                          kmh=float(np.median(ok[:, 2])), slowest=float(np.median(r[:, 3])))
                text += (f"     {len(ok):2d}   {rl['body']:4.2f} ({ok[:, 1].max():4.2f})      {rl['path']:4.2f} "
                         f"({r[:, 0].max():4.2f})   {rl['kmh']:4.0f}     {rl['slowest']:4.0f} | ")
            else:
                text += f"     {len(ok):2d}     -                -             -        - | "
        cells = {}
        for p in physics:
            x = np.array(game[p][k]) if game[p][k] else np.zeros((0, 6))
            lost = (x[:, 3] > WIDE) | (x[:, 4] > SLID)
            under = ~lost & (x[:, 5] < x[lost, 5].max(initial=0.0))
            cells[p] = None
            if under.any():
                i = np.flatnonzero(under)[x[under, 5].argmax()]
                cells[p] = dict(body=float(x[i, 1]), path=float(x[i, 0]), kmh=float(x[i, 5]))
                text += f"{x[i, 1]:5.2f} / {x[i, 0]:4.2f} at {x[i, 5]:4.0f} km/h [{len(x):2d}]      | "
            else:
                text += f"{'none held' if lost.any() else 'held all' if len(x) else '-':>27s} [{len(x):2d}] | "
        print(text)
        rows.append((c, rl, cells))
    print()
    for lo, hi in SPEEDS if real else ():
        sel = [rl for _, rl, _ in rows if rl and rl["body"] >= args.real_floor and lo <= rl["kmh"] < hi]
        if len(sel) >= 3:
            print(f"real, the corners with a body g median of {args.real_floor} or more taken at {lo}-{hi} km/h (n "
                  f"{len(sel)}): body g {np.median([x['body'] for x in sel]):.2f}, path g "
                  f"{np.median([x['path'] for x in sel]):.2f}")
    med = lambda v: float(np.nanmedian(v))
    for p in physics:
        got = [(c, rl, cells[p]) for c, rl, cells in rows if cells[p]]
        print(f"{p}: {len(got)} corners with a limit, body g p25 / p50 / p75 "
              f"{quartiles([x[2]['body'] for x in got])}, path g {quartiles([x[2]['path'] for x in got])}")
        both = [x for x in got if x[1] and x[1]["body"] >= args.real_floor]
        for name, sel in (("all", both), (f"turning {math.degrees(LONG):.0f} deg or more", [
                x for x in both if x[0]["turn"] >= LONG]), ("turning less", [x for x in both if x[0]["turn"] < LONG])):
            if len(sel) >= 3:
                print(f"  where the real body g's median is {args.real_floor} or more, {name} (n {len(sel)}): body g "
                      f"{med([x[2]['body'] for x in sel]):.2f} at {med([x[2]['kmh'] for x in sel]):.0f} km/h against "
                      f"{med([x[1]['body'] for x in sel]):.2f} at {med([x[1]['kmh'] for x in sel]):.0f} (slowest "
                      f"{med([x[1]['slowest'] for x in sel]):.0f}); over the real, p25 / p50 / p75: body g "
                      f"{quartiles([x[2]['body'] / x[1]['body'] for x in sel])}, path g "
                      f"{quartiles([x[2]['path'] / x[1]['path'] for x in sel])}, km/h over the slowest "
                      f"{quartiles([x[2]['kmh'] / x[1]['slowest'] for x in sel])}")


def by_length(d, ds=1.0):
    """The run's path on a `ds` grid of its own length: driveline distances and positions."""
    at = np.maximum.accumulate(d["at"])
    keep = np.concatenate([[True], np.diff(at) > 1e-3])
    at, xy = at[keep], np.column_stack([d["x"][keep], d["y"][keep]])
    length = np.concatenate([[0], np.cumsum(np.hypot(*np.diff(xy, axis=0).T))])
    grid = np.arange(0, length[-1], ds)
    return np.interp(grid, length, at), np.column_stack([np.interp(grid, length, xy[:, k]) for k in (0, 1)])


def demand(args):
    specs, bases = zip(*(spec_of(d) for d in args.dirs))
    found = specs[0]["corners"]
    names = [f"{k + 1} {'race line' if s.get('race') else 'driveline'}" for k, s in enumerate(specs)]
    lines = [by_length(b) for b in bases]
    rows = [[] for _ in found]
    for t, g, at, _, _ in laid(bases[0], args.real, args.stage, args.ref):
        v, a = lateral(t, g)
        ok = ~np.isnan(at) & ~np.isnan(v)
        order = np.argsort(at[ok])
        asked = []
        for line_at, xy in lines:
            # The line at this drive's speed by driveline distance, sampled at 10 Hz of that clock.
            speed = np.maximum(np.interp(line_at, at[ok][order], v[ok][order]), CRAWL)
            clock = np.concatenate([[0], np.cumsum(2.0 / (speed[1:] + speed[:-1]))])
            tick = np.arange(0, clock[-1], 0.1)
            path = np.column_stack([np.interp(tick, clock, xy[:, k]) for k in (0, 1)])
            asked.append((np.interp(tick, clock, line_at), lateral(tick, path)[1]))
        for k, c in enumerate(found):
            real, i = peak(c, at, a)
            if i is not None:
                rows[k].append([real / G, v[i] * 3.6, *(peak(c, s, x)[0] / G for s, x in asked)])
    print("\nper corner: the real drives' path g (1 s fit) and km/h at it, then per line the g it asks at the same "
          "speeds and that over the real, medians over the drives")
    print("    at   R m dir turn |  real g  km/h | " + " | ".join(f"{n[-13:]:>13s}" for n in names))
    table = []
    for c, r in zip(found, rows):
        if len(r) < 3:
            continue
        r = np.array(r)
        m, over = np.nanmedian(r, axis=0), np.nanmedian(r[:, 2:] / r[:, :1], axis=0)
        table.append((c, m, over))
        print(f"{c['at']:7.0f} {c['radius']:5.0f}  {'L' if c['left'] else 'R'} {math.degrees(c['turn']):5.0f} | "
              f"  {m[0]:5.2f}  {m[1]:4.0f} | "
              + " | ".join(f"{m[2 + j]:5.2f}  {over[j]:5.2f} " for j in range(len(names))))
    print()
    near = [x for x in table if x[1][0] >= args.real_floor]
    for name, sel in (("all", near), (f"turning {math.degrees(LONG):.0f} deg or more", [
            x for x in near if x[0]["turn"] >= LONG]), ("turning less", [x for x in near if x[0]["turn"] < LONG])):
        if len(sel) >= 3:
            print(f"corners with a real path g of {args.real_floor} or more, {name} (n {len(sel)}): real "
                  f"{np.median([x[1][0] for x in sel]):.2f} g at {np.median([x[1][1] for x in sel]):.0f} km/h; asked "
                  "over the real, p25 / p50 / p75: "
                  + "; ".join(f"{n} {quartiles([x[2][j] for x in sel])}" for j, n in enumerate(names)))


def circle(args):
    cx, cy = (float(x) for x in args.centre.split(","))
    for path in args.traces:
        radius = float(re.search(r"\.r(\d+)\.trace$", path).group(1))
        with open(path) as f:
            names = f.readline().split()[1:]
            cols = ("time", "speed", "x", "y", "beta", "glong", "glat")
            rows = [[float(v[names.index(n)]) for n in cols] for k, line in enumerate(f) if k % 7 == 0
                    for v in [line.split()] if len(v) == len(names)]
        d = dict(zip(cols, np.array(rows).T))
        d["at"], d["off"] = d["time"], np.abs(np.hypot(d["x"] - cx, d["y"] - cy) - radius)
        s = tenth(d)
        v, path_g = lateral(s["t"], s["xy"])
        along, across = body(s["t"], s["long"], s["lat"])
        index = np.arange(len(v))
        on = index > np.flatnonzero(s["off"] < 1.0)[0]
        gone = np.flatnonzero(on & (s["off"] > OFF_CIRCLE))
        ok = on & (index < (gone[0] if len(gone) else len(v))) & (s["beta"] < SPUN) & ~np.isnan(path_g + across)
        coast = ok & (np.abs(along) < COAST)
        i, j = index[coast][np.abs(across[coast]).argmax()], index[ok][np.abs(path_g[ok]).argmax()]
        print(f"{os.path.basename(path)}: body {abs(across[i]):.3f} g at {v[i] * 3.6:.0f} km/h, "
              f"path {abs(path_g[j]) / G:.3f} g at {v[j] * 3.6:.0f} km/h")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--out", required=True)
    r.add_argument("--rsf-stage", type=int, default=524)
    r.add_argument("--car", default=CAR)
    r.add_argument("--physics", default="experimental,experimental-brush")
    r.add_argument("--levels", default=LEVELS)
    r.add_argument("--top-speed", type=float, default=50.0)
    r.add_argument("--race")
    r.add_argument("--jobs", type=int, default=8)
    d = sub.add_parser("read")
    d.add_argument("dir")
    m = sub.add_parser("demand")
    m.add_argument("dirs", nargs="+")
    c = sub.add_parser("circle")
    c.add_argument("centre")
    c.add_argument("traces", nargs="+")
    for q in (d, m):
        q.add_argument("--real", required=q is m)
        q.add_argument("--stage", default="SS7")
        q.add_argument("--ref", default="Evans")
        q.add_argument("--real-floor", type=float, default=0.8)
    args = ap.parse_args()
    {"run": run, "read": read, "demand": demand, "circle": circle}[args.cmd](args)


if __name__ == "__main__":
    main()
