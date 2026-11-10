"""rally_wot.py: full-throttle acceleration on straights, real rally cars' telemetry against the game's traces.

  python3 -I -B harness/rally_wot.py fetch YEAR EVENT OUT_DIR
  python3 -I -B harness/rally_wot.py table DIR [TRACE...] [--cars NAME=MODEL,...]
  python3 -I -B harness/rally_wot.py fit DIR|TRACE... [--cars NAME=MODEL,...] [--mass KG] [--crr C] [--rho R]
  python3 -I -B harness/rally_wot.py launch DIR|TRACE...

`fetch` asks the WRC's archive (webappsdata.wrc.com, the source of RallyDataJunkie's files) for the YEAR season's
event whose name contains EVENT and writes every stage's merged telemetry to OUT_DIR as
`df_telemetrymergeddata_<stage>_<driver>.csv` (`rally_gg.py`'s format; the data has no stated licence and stays
outside the repository). Seasons 2017 to 2021 carry it with a gear, a throttle, the rpm and the altitude; 2016's has
no altitude.

Samples, both sources: the middle of a window (telemetry 1.5 s, traces 0.5 s) spent in one gear from 3rd up, above
MIN_KMH, at full throttle (98 % up) with no brake, the window's median lateral under LAT g and, with GPS, the course
turning under COURSE deg/s. The acceleration is the speed's change over the window, on the level: plus g times the
grade from the altitude over 3 s (whole metres). The telemetry's speed is the drivetrain's (rpm over speed holds to 1 %
per gear), so a wheel flaring over a crest reads as acceleration: medians, and Huber weights in the fit.

`table`: the median acceleration, g, per gear and 10 km/h bin (10 samples up), the real cars per model (`--cars`, the
drivers' cars: not in the data) and together, then each trace (`rbr-standalone --trace-speed` of a flat-out run on a
flat pad).

`fit`: least squares of `k m a = Pw(rpm) / v - rho CdA v^2 / 2 - Crr m g` for the wheel power Pw at each 500 rpm
from 6000 to 8500 and the drag area CdA: the same rpm comes at different speeds in different gears, which is what
separates the two. `k` is the rotating parts' share (K: the wheels' 3.2 % and 0.10 kg m^2 of engine through each gear,
assumed); the mass (crew and fuel in), Crr and rho are given. The 5-95 % range is a bootstrap over the files. A trace
of the game checks the method: it reads the file's `Cd * A / 1.204` within about 10 % and the power within 3 %.

`launch`: the standing start of each stage or trace, from the last standstill before the car first passes 60 km/h:
the time to MARKS m of path (the telemetry's from its GPS positions, whose first seconds come 0.3 to 1 s apart; a
trace's from the car's speed) and the drivetrain's speed there (the telemetry's `speed`; a trace's mean of the wheels'
`wfl`..`wrr`), the medians over a directory's drives. Stage starts turn after 100 m or so.
"""
import argparse
import base64
import bisect
import csv
import glob
import gzip
import json
import math
import os
import random
import re
import statistics
import time
import urllib.request

import numpy as np

API = "https://webappsdata.wrc.com/srv/wrc/json/api/wrcsrv/"
PULL = "https://webappsdata.wrc.com/srv/fs/pull"
G = 9.80665
MIN_KMH = 90.0
LAT = 0.15
COURSE = 1.5
NODES = list(range(6000, 8501, 500))
K = {3: 1.083, 4: 1.068, 5: 1.058, 6: 1.051}
MARKS = (10, 25, 50, 100)


def get(url):
    time.sleep(0.5)
    with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "curl/8"}), timeout=120) as r:
        return r.read()


def by_id(node, depth):
    return json.loads(get(f"{API}byId?id=%22{node}%22&maxdepth={depth}"))


def meta(node):
    return {m["n"]: m["v"] for m in node.get("_meta", [])}


def fetch(args):
    query = "queryMeta?t=%22Season%22&p=%7B%22n%22%3A%22category%22%2C%22v%22%3A%22WRC%22%7D&maxdepth=1"
    season = next(s for s in json.loads(get(API + query)) if s.get("name") == args.year)
    events = [e for e in by_id(season["_id"], 2)["_dchildren"] if args.event.lower() in e.get("name", "").lower()]
    if len(events) != 1:
        raise SystemExit(f"{args.event}: {[e.get('name') for e in events] or 'no event'} in {args.year}")
    os.makedirs(args.out, exist_ok=True)
    for stage in by_id(events[0]["_id"], 2)["_dchildren"]:
        if stage.get("type") != "Session":
            continue
        for entry in by_id(stage["_id"], 50).get("_dchildren", []):
            merged = meta(entry).get("telemetry_merged")
            driver = "".join(c for c in entry.get("name", "") if c.isalnum())
            path = os.path.join(args.out, f"df_telemetrymergeddata_{stage['name'].upper()}_{driver}.csv")
            if not merged or os.path.exists(path):
                continue
            rows = json.loads(gzip.decompress(base64.b64decode(get(PULL + merged))))["_entries"]
            with open(path, "w", newline="") as handle:
                writer = csv.DictWriter(handle, sorted({k for r in rows for k in r}))
                writer.writeheader()
                writer.writerows(rows)
            print(stage["name"], driver, len(rows), flush=True)


def load_telemetry(path):
    rows = []
    with open(path, newline="") as handle:
        for r in csv.DictReader(handle):
            try:
                rows.append(dict(t=int(r["utx"]) / 1000.0, v=float(r["speed"]), gear=int(float(r["gear"])),
                                 thr=float(r["throttle"]), brk=float(r["brk"]), rpm=float(r["rpm"]),
                                 lat=float(r["accx"]) / 100.0, alt=float(r["altitude"]),
                                 pos=(float(r["lon"]), float(r["lat"]))))
            except ValueError:
                continue
    return sorted((r for r in rows if r["t"] > 1e9), key=lambda r: r["t"])


def load_trace(path):
    rows, last = [], -1.0
    with open(path) as handle:
        col = {name: i for i, name in enumerate(handle.readline().split()[1:])}
        for line in handle:
            f = line.split()
            t = float(f[col["time"]])
            if t - last >= 0.0999:
                last = t
                rows.append(dict(t=t, v=float(f[col["speed"]]) * 3.6, gear=int(f[col["gear"]]) - 1,
                                 thr=float(f[col["thr"]]) * 100.0, brk=float(f[col["brk"]]),
                                 rpm=float(f[col["engine"]]) * 30.0 / math.pi, lat=float(f[col["glat"]]),
                                 wheels=sum(float(f[col[w]]) for w in ("wfl", "wfr", "wrl", "wrr")) * 0.9))
    return rows


def course(p, q):
    return math.atan2((q[0] - p[0]) * math.cos(math.radians(p[1])), q[1] - p[1])


def samples(rows, half):
    """(speed m/s, gear, level acceleration m/s^2, rpm) per sample."""
    t = [r["t"] for r in rows]
    out = []

    def window(r, h):
        return rows[bisect.bisect_left(t, r["t"] - h - 0.01):bisect.bisect_right(t, r["t"] + h + 0.01)]

    for r in rows:
        win = window(r, half)
        span = win[-1]["t"] - win[0]["t"]
        if (not 2 * half - 0.21 <= span <= 2 * half + 0.21 or r["gear"] not in K or r["v"] < MIN_KMH
                or any(w["thr"] < 98 or w["brk"] > 0 or w["gear"] != r["gear"] for w in win)
                or abs(statistics.median(w["lat"] for w in win)) > LAT):
            continue
        acc = (win[-1]["v"] - win[0]["v"]) / 3.6 / span
        if "pos" in r:
            mid = win[len(win) // 2]
            turn = math.degrees(course(mid["pos"], win[-1]["pos"]) - course(win[0]["pos"], mid["pos"]))
            long = window(r, 1.5)
            run = sum((a["v"] + b["v"]) / 7.2 * (b["t"] - a["t"]) for a, b in zip(long, long[1:]))
            if abs((turn + 180) % 360 - 180) / (span / 2) > COURSE or long[-1]["t"] - long[0]["t"] < 2.7 or run < 20:
                continue
            acc += G * (long[-1]["alt"] - long[0]["alt"]) / run
        out.append((r["v"] / 3.6, r["gear"], acc, r["rpm"]))
    return out


def sources(args):
    """{model: {file: samples}} of a telemetry directory (and "all"), or {trace name: {"run": samples}}."""
    if not os.path.isdir(args.paths[0]):
        return {os.path.basename(p): {"run": samples(load_trace(p), 0.25)} for p in args.paths}
    cars = dict(kv.split("=") for kv in args.cars.split(",")) if args.cars else {}
    per = {}
    for path in sorted(glob.glob(os.path.join(args.paths[0], "df_telemetrymergeddata_*.csv"))):
        name = re.search(r"data_(.*)\.csv", path).group(1)
        s = samples(load_telemetry(path), 0.75)
        per.setdefault("all", {})[name] = s
        if name.split("_", 1)[1] in cars:
            per.setdefault(cars[name.split("_", 1)[1]], {})[name] = s
    return per


def table(args):
    real = sources(argparse.Namespace(paths=args.paths[:1], cars=args.cars))
    game = sources(argparse.Namespace(paths=args.paths[1:], cars=None)) if args.paths[1:] else {}
    cells = {name: {} for name in list(real) + list(game)}
    for name, files in list(real.items()) + list(game.items()):
        for v, gear, acc, _ in (s for f in files.values() for s in f):
            cells[name].setdefault((gear, int(v * 3.6 // 10) * 10), []).append(acc / G)
    need = {name: 10 if name in real else 1 for name in cells}
    print("gear km/h   " + "  ".join(f"{name[:22]:>22}" for name in cells))
    for key in sorted({k for name in real for k, v in cells[name].items() if len(v) >= 10}):
        row = [f"{statistics.median(c[key]):.3f} ({len(c[key])})" if len(c.get(key, [])) >= need[n] else "-"
               for n, c in cells.items()]
        print(f"{key[0]}    {key[1]:3d}-{key[1] + 10:3d} " + "  ".join(f"{x:>22}" for x in row))


def solve(s, args):
    a = np.zeros((len(s), len(NODES) + 1))
    y = np.zeros(len(s))
    for i, (v, gear, acc, rpm) in enumerate(s):
        k = min(int((rpm - NODES[0]) // 500), len(NODES) - 2)
        f = (rpm - NODES[k]) / 500.0
        a[i, k], a[i, k + 1], a[i, -1] = (1 - f) / v, f / v, -0.5 * args.rho * v * v
        y[i] = K[gear] * args.mass * acc + args.crr * args.mass * G
    w = np.ones(len(s))
    for _ in range(5):
        x = np.linalg.lstsq(a * w[:, None], y * w, rcond=None)[0]
        res = np.abs(y - a @ x)
        w = np.sqrt(np.minimum(1.0, (1.345 * 1.4826 * np.median(res) + 1e-9) / np.maximum(res, 1e-9)))
    return x, float(np.median(res)) / args.mass / G


def fit(args):
    random.seed(1)
    for name, files in sources(args).items():
        files = {f: [q for q in s if NODES[0] <= q[3] <= NODES[-1]] for f, s in files.items()}
        s = [q for f in files.values() for q in f]
        if len(s) < 100:
            print(f"{name}: {len(s)} samples, too few")
            continue
        x, res = solve(s, args)
        boots = [solve([q for f in random.choices(list(files), k=len(files)) for q in files[f]], args)[0]
                 for _ in range(200 if len(files) > 1 else 0)]
        lo, hi = np.percentile(boots, (5, 95), axis=0) if boots else (x, x)

        def cell(i, scale, digits):
            spread = f" ({lo[i] / scale:.{digits}f}-{hi[i] / scale:.{digits}f})" if boots else ""
            return f"{x[i] / scale:.{digits}f}{spread}"

        gears = {g: sum(q[1] == g for q in s) for g in K}
        print(f"{name}: {len(files)} files, {len(s)} samples (per gear {gears}), median residual {res:.3f} g")
        print(f"  CdA {cell(-1, 1, 2)} m2; wheel power, kW: "
              + "  ".join(f"{n}: {cell(i, 1e3, 0)}" for i, n in enumerate(NODES)))


def launch_marks(rows):
    """[(time s, drivetrain speed km/h)] at each of MARKS, or None without a whole standing start."""
    fast = next((i for i, r in enumerate(rows) if r["v"] > 60), 0)
    start = max((i for i in range(fast) if rows[i]["v"] < 0.5), default=None)
    if start is None:
        return None
    out, path = [], 0.0
    for a, b in zip(rows[start:], rows[start + 1:]):
        if "pos" in a:
            east = (b["pos"][0] - a["pos"][0]) * math.cos(math.radians(a["pos"][1]))
            step = math.hypot(east, b["pos"][1] - a["pos"][1]) * 111320.0
        else:
            step = (a["v"] + b["v"]) / 7.2 * (b["t"] - a["t"])
        while len(out) < len(MARKS) and path + step >= MARKS[len(out)]:
            f = (MARKS[len(out)] - path) / step
            wheel = [r.get("wheels", r["v"]) for r in (a, b)]
            out.append((a["t"] + f * (b["t"] - a["t"]) - rows[start]["t"], wheel[0] + f * (wheel[1] - wheel[0])))
        path += step
        if b["t"] - a["t"] > 1.2 or len(out) == len(MARKS):
            break
    return out if len(out) == len(MARKS) else None


def launch(args):
    print("time s / drivetrain km/h at " + ", ".join(f"{m} m" for m in MARKS))
    if os.path.isdir(args.paths[0]):
        files = sorted(glob.glob(os.path.join(args.paths[0], "df_telemetrymergeddata_*.csv")))
        runs = [m for m in (launch_marks(load_telemetry(p)) for p in files) if m]
        marks = [tuple(statistics.median(r[i][j] for r in runs) for j in (0, 1)) for i in range(len(MARKS))]
        print(f"{len(runs)} starts, medians: " + "  ".join(f"{t:.2f} / {v:.0f}" for t, v in marks))
    else:
        for path in args.paths:
            marks = launch_marks(load_trace(path)) or []
            print(os.path.basename(path) + ": " + "  ".join(f"{t:.2f} / {v:.0f}" for t, v in marks))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    f = sub.add_parser("fetch")
    f.add_argument("year")
    f.add_argument("event")
    f.add_argument("out")
    for name in ("launch", "table", "fit"):
        p = sub.add_parser(name)
        p.add_argument("paths", nargs="+")
        p.add_argument("--cars")
    p.add_argument("--mass", type=float, default=1390.0)
    p.add_argument("--crr", type=float, default=0.015)
    p.add_argument("--rho", type=float, default=1.17)
    args = parser.parse_args()
    {"fetch": fetch, "launch": launch, "table": table, "fit": fit}[args.command](args)


if __name__ == "__main__":
    main()
