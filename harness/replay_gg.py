"""replay_gg.py: a player's drive (a launcher `.rrp` replay) in the real telemetry's terms, beside autopilot runs.

  python3 -I -B harness/replay_gg.py FILE... --out DIR [--bin PROGRAM] [--speed-lag S] [--every N] [--jobs N]
      [--no-tyre] [--csv OUT]

Each FILE is a replay (`.rrp`, copy it out of the launcher's folder first) or an autopilot trace (`.trace`, with its
`.log` and, for the tyre figures, a `--tyre-probe` dump `.probe` beside it; `rally_gg.py run`, `limit_pace.py`).

A replay runs again through `rbr-standalone --live <the replay's stepping arguments> --replay FILE --replay-pace free
--replay-trace DIR/<name>.trace --tyre-probe DIR/<name>.probe --tyre-probe-every N` (N 144 steps, about 0.2 s), on
PROGRAM (by default target/release/rbr-standalone; `--bin replay` the program the replay's header names, the build
that drove, which takes `--tyre-probe` only from this change on), its stderr to DIR/<name>.log; a log whose replay
ended "all equal" is not run again. The physics, car and stage are the header's (the stepping command:
`--physics`, ngp when absent, and its `--tyre-tuning`, printed beside it; `--rsf-car` or `--car-pack`; `--rsf-stage`
or `--stage`). A replay steps exactly
only on a build whose physics matches the drive's (each frame's digest is compared): on another the run stops at the
first frame that differs, and the figures below cover the frames up to it (printed as DIVERGED with the frame).

Per file, with rally_gg.py's definitions (its `summary`: 10 Hz from 0.1 s means, 3-sample median, MOVING km/h):

- time (the race time at the finish line, else the traced time), km driven (the speed's integral), the stage
  card's distance (replays), outside assistance (the game's recoveries) and position jumps over 2 m in a step, the
  largest distance off the driveline (m, horizontal; replays: from the car's driveline parameter on the stage's
  `.trk` spline, as the autopilot's `off`; autopilot traces: `off`);
- lateral 1 s p50/p90/p99 and the g-g p95 |lat| per speed bin, as an accelerometer (`glat`: the specific force on
  the body's +X axis, the velocity's change less gravity, the autopilot trace's own column) and kinematic (v times
  the course's rate, the path's own lateral g); braking (the LONGEST longest events from the speed, their 0.5 s
  peak), the speed through `--speed-lag S`;
- tyre use (`rbr-tyre-lab` example `limit_frames` on the probe dumps, its peak μ_y at each wheel's load and speed for
  the physics' tyre: `brush` under experimental-brush, untuned, else NGP's): each axle's Σ|F| / Σ peak·Fz and each wheel's
  |F|/Fz over its peak (wheels over LOADED N), the mean over the hardest 5 % of moving moments (dumps where the car
  moves over MOVING, ranked by the car's Σ|F|/ΣFz) and the median over all moving moments; the car's alike.

Needs target/release/rbr-standalone (or --bin) and target/release/examples/limit_frames (`cargo build --release -p
rbr-tyre-lab --example limit_frames`). RBR_NGP_DLL: the DLL beside the replay's `--ngp-physics` (the launcher's).
"""
import argparse
import concurrent.futures
import csv
import math
import os
import re
import statistics
import struct
import subprocess
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rally_gg  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(REPO, "target", "release", "rbr-standalone")
LIMIT_FRAMES = os.path.join(REPO, "target", "release", "examples", "limit_frames")
G = rally_gg.G
GRAVITY = 9.81  # the physics' (autopilot `specific_force`)
JUMP = 2.0  # m in one step beyond the speed's own travel: a recovery or reset moved the car
HARDEST = 0.05
LOADED = 500.0  # N: a wheel's own ratio only above it (an unloaded wheel's |F|/Fz is noise)
PARTS = 24  # limit_frames runs per probe


def header_args(path):
    """The viewer's arguments a replay's header keeps (`rbr_recording::replay`)."""
    with open(path, "rb") as f:
        if f.read(4) != b"RRP1":
            sys.exit(f"{path}: not a replay (RRP1)")
        args = []
        for _ in range(struct.unpack("<I", f.read(4))[0]):
            args.append(f.read(struct.unpack("<I", f.read(4))[0]).decode())
    return args


def flag(args, name, default=None):
    return args[args.index(name) + 1] if name in args else default


def replay_info(path):
    """(program, stepping arguments, info dict) of a replay: the arguments after the viewer's `--live PROGRAM`."""
    args = header_args(path)
    live = args.index("--live")
    program, step = args[live + 1], args[live + 2:]
    car = flag(step, "--rsf-car") or os.path.basename(flag(step, "--car-pack", "") or "") or flag(args, "--car", "?")
    stage = flag(step, "--rsf-stage") or os.path.basename(flag(step, "--stage", "?"))
    return program, step, {"physics": flag(step, "--physics", "ngp"), "tuning": flag(step, "--tyre-tuning", ""),
                           "car": car, "stage": stage,
                           "title": flag(args, "--title", ""), "program": program}


def run_replay(path, out, program, step, every, probe):
    """Runs the replay into DIR/<name>.trace (and .probe); returns (trace, probe or None, log text)."""
    name = os.path.join(out, os.path.basename(path).removesuffix(".rrp"))
    trace, log = name + ".trace", name + ".log"
    dump = name + ".probe" if probe else None
    done = os.path.exists(log) and "all equal" in open(log).read()
    if not done or (dump and not os.path.exists(dump)):
        if not os.path.exists(program):
            sys.exit(f"{program}: no such program (build it: cargo build --release -p rbr-standalone)")
        cmd = [program, *step, "--replay", path, "--replay-pace", "free", "--replay-trace", trace]
        if dump:
            cmd += ["--tyre-probe", dump, "--tyre-probe-every", str(every)]
        env = dict(os.environ)
        dll = os.path.join(os.path.dirname(flag(step, "--ngp-physics", "")), "NGP.dll")
        if os.path.exists(dll):
            env["RBR_NGP_DLL"] = dll
        # The real-time protocol: a first input and a resume start the replay; stdin stays open until it ends.
        start = bytes(28) + b"INP2"  # an input payload at rest (`rbr_recording::input`)
        with open(log, "w") as err:
            p = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=err, env=env)
            p.stdin.write(b"RTI1" + struct.pack("<I", 1) + start + b"RTR1" + struct.pack("<I", 1) + start)
            p.stdin.flush()
            p.wait()
            p.stdin.close()
    return trace, dump, open(log).read()


def driveline(step):
    """The stage's driveline records (position, tangent) from its `.trk` (3, 0x14) chunk, or None."""
    stage = flag(step, "--stage")
    if stage is None:
        return None
    path = stage if os.path.isabs(stage) else os.path.join(flag(step, "--data", ""), "Maps", stage)
    try:
        data = open(path + ".trk", "rb").read()
    except OSError:
        return None
    at = 0
    while at < len(data):
        hlen = struct.unpack_from("<I", data, at)[0]
        head = data[at + 4:at + 4 + hlen]
        size = struct.unpack_from("<I", data, at + 4 + hlen)[0]
        body = at + 8 + hlen
        if hlen == 8 and struct.unpack("<II", head) == (3, 0x14):
            count = struct.unpack_from("<I", data, body)[0]
            rec = np.frombuffer(data, dtype="<f4", count=count * 8, offset=body + 4).reshape(count, 8)
            return rec[:, 0:3].astype(float), rec[:, 3:6].astype(float)
        at = body + size
    return None


def hermite(p0, m0, p1, m1, t):
    t = t[:, None]
    t2, t3 = t * t, t * t * t
    return (2 * t3 - 3 * t2 + 1) * p0 + (t3 - 2 * t2 + t) * m0 + (-2 * t3 + 3 * t2) * p1 + (t3 - t2) * m1


def line_points(line, u):
    """The driveline's points at parameters `u` (record index plus fraction, `CCar` +0x17c), the autopilot's `point`
    (rbr-standalone `autopilot/mod.rs`) with its tangent shortening for looping segments."""
    pos, tan = line
    last = len(pos) - 1
    u = np.clip(u, 0, last)
    i = np.minimum(u.astype(int), last - 1)
    t = u - i
    p0, p1, m0, m1 = pos[i], pos[i + 1], tan[i], tan[i + 1]
    chord = np.linalg.norm(p1 - p0, axis=1)
    # runs_back: how far the segment runs back along its chord, sampled at 16 points
    unit = (p1 - p0) / np.maximum(chord, 1e-9)[:, None]
    ahead, back = np.zeros(len(u)), np.zeros(len(u))
    for k in range(1, 17):
        along = np.sum((hermite(p0, m0, p1, m1, np.full(len(u), k / 16)) - p0) * unit, axis=1)
        ahead = np.maximum(ahead, along)
        back = np.maximum(back, ahead - along)
    loops = back > 1.5

    def fit(m):
        n = np.linalg.norm(m, axis=1)
        long = (n > chord) & (loops | (n > 8.0 * chord))
        return m * np.where(long, chord / np.maximum(n, 1e-9), 1.0)[:, None]

    return hermite(p0, fit(m0), p1, fit(m1), t)


def columns(path):
    """The trace's numeric rows as an array and its columns by name."""
    with open(path) as f:
        names = f.readline().split()[1:]
        rows = [line.split() for line in f]
    rows = [r for r in rows if len(r) == len(names)]
    return np.array(rows, dtype=float), {n: i for i, n in enumerate(names)}


def rotate(q, v):
    """`CarPose::rotate`: body vector v by the quaternions q (x y z w), per row."""
    x, y, z, w = q.T
    m = np.array([[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
                  [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
                  [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]])
    return np.einsum("ijn,j->ni", m, np.asarray(v, dtype=float))


def per_step(a, c, kind):
    """(t, speed m/s, brake, glong, glat, kinematic lat g, x, y) per traced row. Replays: the specific force from the
    velocity's change over the row's time less gravity on the body's forward (-Y) and +X axes; kinematic from the
    horizontal velocity's turn. Autopilot traces: their glong/glat, kinematic from the course (yaw + beta)."""
    t = a[:, c["time"]]
    dt = np.diff(t, prepend=np.nan)
    if kind == "replay":
        v = a[:, [c["vx"], c["vy"], c["vz"]]]
        q = a[:, [c["qx"], c["qy"], c["qz"], c["qw"]]]
        acc = np.diff(v, axis=0, prepend=np.nan * v[:1]) / dt[:, None] + [0, 0, GRAVITY]
        glong = np.sum(acc * rotate(q, [0, -1, 0]), axis=1) / GRAVITY
        glat = np.sum(acc * rotate(q, [1, 0, 0]), axis=1) / GRAVITY
        vh = np.hypot(v[:, 0], v[:, 1])
        kin = (v[:, 0] * acc[:, 1] - v[:, 1] * acc[:, 0]) / np.maximum(vh, 0.5) / G
        speed = np.linalg.norm(v, axis=1)
    else:
        speed, glong, glat = a[:, c["speed"]], a[:, c["glong"]], a[:, c["glat"]]
        if "yaw" in c:
            course = np.unwrap(a[:, c["yaw"]] + a[:, c["beta"]])
            kin = speed * np.diff(course, prepend=np.nan) / dt / G
        else:
            kin = np.full(len(t), np.nan)
    return t, speed, a[:, c["brk"]], glong, glat, kin


def binned(t, speed, brk, glong, glat, kin):
    """10 Hz rows of 0.1 s means, as `rally_gg.load_trace` (its first bin dropped): (t, km/h, brake, long, lat, kin)."""
    key = (t * rally_gg.RATE).astype(int)
    ok = np.isfinite(glong) & np.isfinite(kin)
    keys, inv = np.unique(key[ok], return_inverse=True)
    n = np.bincount(inv)
    cols = [np.bincount(inv, weights=x[ok]) / n for x in (speed * 3.6, brk, glong, glat, kin)]
    return [((k + 0.5) / rally_gg.RATE, *(float(col[j]) for col in cols)) for j, k in enumerate(keys)][1:]


def tyre_use(probe, model, jobs, speed_at):
    """Each wheel's |F|/Fz over its peak μ_y at its load (`limit_frames`), hardest 5 % and median of moving moments."""
    if not os.path.exists(LIMIT_FRAMES):
        sys.exit(f"{LIMIT_FRAMES}: cargo build --release -p rbr-tyre-lab --example limit_frames")
    out = probe + f".{model}.lf"
    if not os.path.exists(out) or os.path.getmtime(out) < os.path.getmtime(probe):
        steps = [int(line.split()[2]) for line in open(probe) if line.startswith("# step")]
        bounds = np.linspace(steps[0], steps[-1] + 1, PARTS + 1).astype(int)
        # In parts of the steps, each kept as it ends: a run cut short goes on from the parts done.
        parts = [(f"{out}.{lo}-{hi}", [LIMIT_FRAMES, "--probe", probe, "--from", str(lo), "--to", str(hi - 1),
                                       "--model", model]) for lo, hi in zip(bounds, bounds[1:])]

        def part(job):
            name, cmd = job
            if not os.path.exists(name):
                text = subprocess.run(cmd, capture_output=True, text=True, check=True).stdout
                with open(name + ".tmp", "w") as f:
                    f.write(text)
                os.replace(name + ".tmp", name)
            return name

        with concurrent.futures.ThreadPoolExecutor(jobs) as pool:
            names = list(pool.map(part, parts))
        with open(out, "w") as f:
            f.write("".join(open(name).read() for name in names))
        for name in names:
            os.remove(name)
    moments = {}
    for line in open(out):
        if line.startswith("#"):
            continue
        x = line.split()
        fz, fy, fx, peak, v = float(x[2]), float(x[7]), float(x[8]), float(x[18]), float(x[19])
        moments.setdefault(int(x[0]), {})[int(x[1])] = (fz, math.hypot(fy, fx) * fz, peak, v)
    moving = [m for step, m in moments.items() if speed_at(step) * 3.6 > rally_gg.MOVING]
    if not moving:
        return None
    car_g = [sum(w[1] for w in m.values()) / sum(w[0] for w in m.values()) for m in moving]
    use = [sum(w[1] for w in m.values()) / sum(w[2] * w[0] for w in m.values()) for m in moving]
    order = np.argsort(car_g)[::-1][:max(1, int(len(moving) * HARDEST))]
    hard = [moving[i] for i in order]

    def ratio(ms, k):
        return [m[k][1] / (m[k][2] * m[k][0]) for m in ms if k in m and m[k][0] > LOADED]

    def axle(ms, ks):
        return [sum(m[k][1] for k in ks if k in m) / sum(m[k][2] * m[k][0] for k in ks if k in m)
                for m in ms if any(k in m for k in ks)]

    res = {"tyre_moments": len(moving), "car_g_hard": statistics.fmean(car_g[i] for i in order),
           "use_hard": statistics.fmean(use[i] for i in order), "use_med": statistics.median(use)}
    for name, ks in (("front", (0, 1)), ("rear", (2, 3))):
        res[f"{name}_hard"] = statistics.fmean(axle(hard, ks) or [float("nan")])
        res[f"{name}_med"] = statistics.median(axle(moving, ks) or [float("nan")])
    for k, w in enumerate(("fl", "fr", "rl", "rr")):
        res[f"{w}_hard"] = statistics.fmean(ratio(hard, k) or [float("nan")])
        res[f"{w}_med"] = statistics.median(ratio(moving, k) or [float("nan")])
    return res


def analyse(path, args):
    if path.endswith(".rrp"):
        program, step, info = replay_info(path)
        info["program"] = program if args.bin == "replay" else args.bin
        trace, probe, log = run_replay(path, args.out, info["program"], step, args.every, not args.no_tyre)
        kind, line = "replay", driveline(step)
        status = (re.findall(r"--replay: (.*)", log) or re.findall(r"(the replay differs at frame \d+[^\n]*)", log)
                  or ["no result line (see the log)"])[-1]
        if "differs" in status:
            status = "DIVERGED: " + status
        finish = re.findall(r"the finish line, race time ([\d.]+)", log)
    else:
        trace, probe = path, path.removesuffix(".trace") + ".probe"
        logpath = path.removesuffix(".trace") + ".log"
        log = open(logpath).read() if os.path.exists(logpath) else ""
        phys = [p for p in ("experimental-brush", "experimental", "ngp") if f".{p}." in os.path.basename(path)]
        car = re.findall(r"rsf car \d+ ([^,]+?) \(", log)
        stage = re.findall(r"weather: [^,]*, (track-\S+?),", log)
        info = {"physics": phys[0] if phys else "?", "tuning": "", "car": car[0] if car else "?",
                "stage": stage[0] if stage else "?", "title": "",
                "program": "autopilot"}
        kind, line, status = "autopilot", None, "autopilot run"
        finish = re.findall(r"EndRace \{ time: ([\d.]+)", log)
        probe = probe if os.path.exists(probe) and not args.no_tyre else None
    if not os.path.exists(trace) or os.path.getsize(trace) < 4096:
        print(f"{os.path.basename(path)}: NOT RUN: {log.strip().splitlines()[0] if log.strip() else 'no log'}")
        return None
    a, c = columns(trace)
    t, speed, brk, glong, glat, kin = per_step(a, c, kind)
    pos = a[:, [c["x"], c["y"], c["z"]]]
    if kind == "replay":
        started = a[:, c["started"]] > 0
        off = (np.hypot(*(pos[:, :2] - line_points(line, a[:, c["param"]])[:, :2]).T) if line is not None
               else np.full(len(t), np.nan))
        card = float(a[-1, c["dist"]])
    else:
        started, off, card = np.ones(len(t), bool), a[:, c["off"]], float("nan")
    dt = np.diff(t, prepend=t[0])
    step_move = np.linalg.norm(np.diff(pos, axis=0, prepend=pos[:1]), axis=1)
    jumps = int(np.sum(step_move > JUMP + speed * dt))
    moving = speed * 3.6 > rally_gg.MOVING
    rows = binned(t, speed, brk, glong, glat, kin)
    acc = rally_gg.summary([r[:5] for r in rows], "trace", args.speed_lag)
    k = rally_gg.summary([r[:4] + (r[5],) for r in rows], "trace", args.speed_lag)
    res = {"file": os.path.basename(path), **info, "status": status,
           "time_s": float(finish[-1]) if finish else float(t[started][-1] - t[started][0]) if started.any() else 0.0,
           "finished": bool(finish), "km": float(np.sum(speed * dt)) / 1000, "card_m": card,
           "assistance": len(re.findall(r"outside assistance", log)), "jumps": jumps,
           "off_max": float(np.nanmax(off[moving])) if moving.any() else float("nan"),
           "off_p99": float(np.nanpercentile(off[moving], 99)) if moving.any() else float("nan")}
    res.update(acc)
    res.update({f"kin_{key}": k[key] for key in ("lat_p50", "lat_p90", "lat_p99")})
    res.update({f"kin_gg{lo}": k[f"gg{lo}"] for lo in rally_gg.BINS[:-1]})
    model = "brush" if info["physics"] == "experimental-brush" else "ngp"
    steps = a[:, c["step"]]
    res["tyre"] = (tyre_use(probe, model, args.jobs,
                            lambda n: speed[max(0, np.searchsorted(steps, n, side="right") - 1)]) if probe else None)
    return res


def report(results):
    for o in results:
        tuning = f" (tyre tuning {o['tuning']})" if o["tuning"] else ""
        print(f"{o['file']}: {o['title'] or o['car']} | stage {o['stage']} | physics {o['physics']}{tuning} | "
              f"{o['program']}")
        print(f"  {o['status']}")
        print(f"  time {o['time_s']:.2f} s{'' if o['finished'] else ' (no finish)'} | {o['km']:.2f} km driven | "
              f"card {o['card_m']:.0f} m | outside assistance {o['assistance']} | jumps {o['jumps']} | "
              f"off driveline max {o['off_max']:.1f} m, p99 {o['off_p99']:.1f} m")
        print(f"  v mean/max {o['vmean']:.0f}/{o['vmax']:.0f} km/h | braking {o['events']} events, speed all/16 "
              f"{o['brk_speed_all']:.2f}/{o['brk_speed_16']:.2f} g, acc 16 {o['brk_acc_16']:.2f}, peak 0.5 s med 16 "
              f"{o['brk_peak_16']:.2f} g")
        print(f"  lateral 1 s p50/p90/p99: glat {o['lat_p50']:.2f}/{o['lat_p90']:.2f}/{o['lat_p99']:.2f} g, "
              f"kinematic {o['kin_lat_p50']:.2f}/{o['kin_lat_p90']:.2f}/{o['kin_lat_p99']:.2f} g")
        cells = [f"{lo}-{hi}: {o[f'gg{lo}'][0]} {o[f'gg{lo}'][1]:.2f}/{o[f'kin_gg{lo}'][1]:.2f}/"
                 f"{o[f'gg{lo}'][2]:.2f}" for lo, hi in zip(rally_gg.BINS, rally_gg.BINS[1:]) if o[f"gg{lo}"][0]]
        print("  g-g p95 (n, |glat|/|kin|/decel): " + "; ".join(cells))
        ty = o["tyre"]
        if ty:
            wheels = " ".join(f"{w} {ty[f'{w}_hard']:.2f}/{ty[f'{w}_med']:.2f}" for w in ("fl", "fr", "rl", "rr"))
            print(f"  tyre |F|/(peak Fz), hardest 5 % / median of {ty['tyre_moments']} moments: front "
                  f"{ty['front_hard']:.2f}/{ty['front_med']:.2f} rear {ty['rear_hard']:.2f}/{ty['rear_med']:.2f}; "
                  f"{wheels}; car "
                  f"{ty['use_hard']:.2f}/{ty['use_med']:.2f} (Σ|F|/ΣFz {ty['car_g_hard']:.2f} in the hardest)")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("files", nargs="+")
    parser.add_argument("--out", required=True, help="the replays' traces, probes and logs")
    parser.add_argument("--bin", default=BIN, help="rbr-standalone to run the replays on (`replay`: the program the "
                        "replay's header names; it needs this branch's --tyre-probe with --replay-trace)")
    parser.add_argument("--speed-lag", type=float, default=0.0, help="s, as rally_gg.py stats")
    parser.add_argument("--every", type=int, default=144, help="steps between tyre probe dumps")
    parser.add_argument("--jobs", type=int, default=6, help="limit_frames processes")
    parser.add_argument("--no-tyre", action="store_true")
    parser.add_argument("--csv")
    args = parser.parse_args()
    os.makedirs(args.out, exist_ok=True)
    results = [r for r in (analyse(os.path.abspath(f), args) for f in args.files) if r]
    report(results)
    if args.csv and results:
        flat = []
        for o in results:
            row = {key: v for key, v in o.items() if key != "tyre" and not isinstance(v, tuple)}
            row.update({f"{key}_lat": v[1] for key, v in o.items() if isinstance(v, tuple)})
            row.update(o["tyre"] or {})
            flat.append(row)
        keys = list(dict.fromkeys(key for row in flat for key in row))
        with open(args.csv, "w", newline="") as f:
            w = csv.DictWriter(f, keys)
            w.writeheader()
            w.writerows(flat)


if __name__ == "__main__":
    main()
