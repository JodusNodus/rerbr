"""handling.py: open-loop handling manoeuvres on a flat pad, per car and physics, through the Rust autopilot.

  python3 -I -B harness/handling.py --out DIR [--physics ngp,experimental,experimental-brush]
      [--cars Ford_Fiesta_Rally2_ngp7,VW_Golf_II_GTI_16V_GrpA_ngp6,Ford_Escort_Mk_II_RS_Grp4_ngp6]
      [--only ramp,step,sine,lift,brake,power] [--speed 80] [--sine-speeds 80,110] [--jobs 6] [--analyse-only]
      [--keep-traces] [--stage track-85_M] [--material 37] [--bin PATH]

Each run is `rbr-standalone --autopilot --boot --flat-pad 37 --manoeuvre KIND,KMH,AMP[,EXTRA] --trace-speed`: the
stage's ground is replaced by a flat, level pad of one material (a 2 km square at the start's height, no scenery
collision, dry tarmac 37 by default), the car holds its start heading and a speed controller brings it to the entry
speed in the gear the shift points give, and when the speed has held for 2 s the inputs go open loop on a fixed
schedule (`crates/rbr-standalone/src/autopilot/pad.rs`; the steering is the exact float, not the controller's 8-bit
steps). Steering is a fraction of full lock, always to the left; lateral g, yaw rate and the turn sideslip are
positive in the turn (the turn sideslip is the angle from the car's forward axis to its velocity, positive when the
velocity points to the outside of the turn). Metrics are over the run's seconds t (0 at the run's start; the traces
are dropped after the analysis unless --keep-traces, `DIR/*.json` hold the metrics).

Manoeuvres (entry speed --speed, 80 km/h; the amplitudes of the circle manoeuvres come from the ramp steer's):
  ramp      constant speed, the steering rising 0.015 per s (about 0.1 g/s) to full lock. Peak g: the highest 0.25 s
            mean of the lateral g (a peak is one the g then falls 3 % from); steer at 0.4 g and at 0.9 of the peak; the
            turn sideslip at the peak; dg/dt between 0.2 and 0.8 of the peak; limit: OS when the turn sideslip passes
            10 deg within 3 s after the peak (the rear axle saturated first and the car rotates), else US (the front
            did, the car holds the g with a small sideslip and runs wide), beta after is that sideslip's maximum.
  step      ISO 7401: throttle held at the speed controller's mean, steering ramped in 0.15 s to the steer of 0.4 g and
            of 0.9 of the peak. Steady state is the mean of 3 to 4 s. g: its value; t90 yaw: the yaw rate's first 0.04 s
            mean to 0.9 of the steady state (from the ramp's middle); os: its peak over the steady state, %; beta: the
            steady turn sideslip; t90 g: the 0.1 s mean lateral g reaching 0.9 of its steady state.
  sine      FMVSS 126: throttle and brake released at t 0, steering one 0.7 Hz sine cycle (left first) with a 0.5 s
            dwell at the second peak (the steering is zero again at 1.93 s), at a ladder of amplitudes (fractions of
            lock), at 80 and 110 km/h. r1.00, r1.75: the yaw rate 1.0 s and 1.75 s after the steering returns to zero
            as a % of the peak yaw rate (both absolute; FMVSS 126 asks under 35 % and 20 %); beta: the peak |sideslip|
            within 4 s; spin: |sideslip| over 60 deg. First spin: the lowest amplitude whose nominal run spins.
  lift      ISO 9816: steady circle of the steer of 0.9 of the peak (1.5 s into it, 5 s on it; where the nominal run's
            circle is not steady, a yaw rate that drifts over 10 % in 2 s or a sideslip over 20 deg, the share steps down
            0.9, 0.8 ... 0.4, and every physics of a car takes the lowest share any of them needed), then the throttle to zero
            in 0.1 s, steering held. dr, dbeta: the yaw rate (deg/s) and turn sideslip (deg) change from the 0.5 s before
            the run, at 1 s (0.1 s mean), and their peaks within 4 s (the larger of the signed extremes); dR: the path
            radius v/r change at 1 s, %.
  brake     ISO 7975: the same circle, then throttle to zero and the brake pedal to P in 0.1 s,
            steering held, clutch and gear as at entry. P is light, medium or hard: 0.3, 0.6 or 0.9 of the car's
            straight-line limit pedal (the lowest pedal of a straight brake from the entry speed, `line` runs under
            the first physics, within 3 % of the best deceleration over 0.3 to 1.0 s). r1/r0: the yaw rate ratio at 1 s; beta1: the turn
            sideslip at 1 s; beta peak and spin as above.
  power     the circle of the steer of 0.8 of the peak, then full throttle in 0.1 s, steering held. As lift.
Outcomes near the limit flip between neighbouring inputs, so sine, lift, brake and power run each case at the entry
speed and at +-2 km/h: the table shows the nominal run's values, a [min..max] of the headline metric over the three
and the spins out of three (the lowest-amplitude spin ignores the neighbours).
target/release/rbr-standalone must be built; RBR_DATA, RBR_RSF and RBR_NGP_PHYSICS come from dev.env."""
import argparse
import bisect
import concurrent.futures
import json
import math
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import dev_env  # noqa: E402,F401  (dev.env: RBR_DATA, RBR_RSF, RBR_NGP_PHYSICS)

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(REPO, "target", "release", "rbr-standalone")
CARS = "Ford_Fiesta_Rally2_ngp7,VW_Golf_II_GTI_16V_GrpA_ngp6,Ford_Escort_Mk_II_RS_Grp4_ngp6"
PHYSICS = "ngp,experimental,experimental-brush"
RATE = 0.015  # steering per s of the ramp steer
SPUN = 60.0  # deg of body sideslip
SLOW = 3.0  # m/s: under it the sideslip is not counted
OVERSTEER = 10.0  # deg of turn sideslip within 3 s of the g peak: the rear axle has let go
LADDER = [0.1, 0.15, 0.2, 0.3, 0.4, 0.55, 0.75, 1.0]  # sine dwell amplitudes, fractions of lock
DELTAS = [0.0, -2.0, 2.0]  # km/h around the nominal entry speed; the nominal first
SINE_END = 1 / 0.7 + 0.5  # s: the steering is zero again
FRACTIONS = [0.9, 0.8, 0.7, 0.6, 0.5, 0.4]  # shares of the peak g a circle manoeuvre may start from, highest first
MEAN = 0.25  # s: the lateral g's moving mean
PAD_HALF = 950.0  # m: the pad is 2 km square about the start; a run farther than this from it is void
LINE_PEDALS = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0]  # the straight-line brake sweep
LEVELS = dict(light=0.3, medium=0.6, hard=0.9)  # the brake in turn's pedals, as a share of the straight-line limit's


def short(car):
    return car.split("_ngp")[0].replace("Ford_", "").replace("VW_", "").replace("_Grp4", "").replace("_GrpA", "")


class Run:
    """A trace's series from the run's start (seconds t from it), with moving means by time."""

    def __init__(self, path):
        t, v, thr, brk, gear, steer, glat, glong, yaw, beta, since, px, py = ([] for _ in range(13))
        with open(path) as handle:
            for line in handle:
                if line.startswith("#") or not line.strip():
                    continue
                f = line.split()
                t.append(float(f[1]))
                v.append(float(f[2]))
                thr.append(float(f[3]))
                brk.append(float(f[4]))
                gear.append(int(f[5]))
                steer.append(-float(f[11]))
                glat.append(float(f[19]))
                glong.append(float(f[18]))
                px.append(float(f[7]))
                py.append(float(f[8]))
                yaw.append(float(f[21]))
                beta.append(-float(f[22]))
                since.append(float(f[20]))
        start = next((i for i, s in enumerate(since) if s >= 0.0), None)
        self.ok = start is not None
        if not self.ok:
            return
        for i in range(1, len(yaw)):
            yaw[i] = yaw[i - 1] + (yaw[i] - yaw[i - 1] + math.pi) % (2 * math.pi) - math.pi
        dt = (t[-1] - t[0]) / (len(t) - 1)
        k = max(1, round(0.02 / dt))
        rate = [(yaw[min(i + k, len(t) - 1)] - yaw[max(i - k, 0)]) / (t[min(i + k, len(t) - 1)] - t[max(i - k, 0)])
                for i in range(len(t))]
        self.t = [x - t[start] for x in t[start:]]
        self.series = dict(v=v[start:], thr=thr[start:], brk=brk[start:], gear=gear[start:], steer=steer[start:],
                           g=glat[start:], gl=glong[start:], r=[math.degrees(x) for x in rate[start:]],
                           beta=[math.degrees(x) for x in beta[start:]])
        self.extent = max(math.hypot(x - px[start], y - py[start]) for x, y in zip(px[start:], py[start:]))
        lead = max(0, start - round(2.5 / dt))
        self.dt = dt
        self.before = dict(r=[math.degrees(x) for x in rate[lead:start]], beta=[math.degrees(x) for x in beta[lead:start]],
                           g=glat[lead:start], v=v[lead:start])
        self.sums = {}
        for name, xs in self.series.items():
            acc = [0.0]
            for x in xs:
                acc.append(acc[-1] + x)
            self.sums[name] = acc

    def mean(self, name, a, b):
        """The mean of a series over t in [a, b] (the sample at a if the span is shorter than a step)."""
        i = bisect.bisect_left(self.t, a)
        j = max(i + 1, bisect.bisect_right(self.t, b))
        j = min(j, len(self.t))
        i = min(i, j - 1)
        s = self.sums[name]
        return (s[j] - s[i]) / (j - i)

    def at(self, name, t, w=0.05):
        return self.mean(name, t - w, t + w)

    def trailing(self, name, t):
        """The mean over the `MEAN` s ending at t."""
        return self.mean(name, t - MEAN, t)

    def first(self, test, a=0.0, b=None):
        """The first sample time in [a, b] where test(index) holds, else None."""
        i = bisect.bisect_left(self.t, a)
        j = len(self.t) if b is None else bisect.bisect_right(self.t, b)
        return next((self.t[k] for k in range(i, j) if test(k)), None)

    def peak(self, name, a, b, key=abs):
        i, j = bisect.bisect_left(self.t, a), bisect.bisect_right(self.t, b)
        return max(self.series[name][i:j], key=key)

    def spun(self, a=0.0, b=None):
        """|sideslip| over `SPUN` deg while faster than `SLOW` m/s (it is noise at a crawl)."""
        i, j = bisect.bisect_left(self.t, a), len(self.t) if b is None else bisect.bisect_right(self.t, b)
        return any(abs(x) > SPUN and v > SLOW for x, v in zip(self.series["beta"][i:j], self.series["v"][i:j]))


def trailing_series(run):
    """The 0.25 s trailing mean of the lateral g at every sample."""
    s, n = run.sums["g"], len(run.t)
    out, j = [], 0
    for i in range(n):
        while run.t[j] < run.t[i] - MEAN:
            j += 1
        out.append((s[i + 1] - s[j]) / (i + 1 - j))
    return out


def analyse_ramp(run):
    g = trailing_series(run)
    ip = max(range(len(g)), key=lambda i: g[i])
    pk = g[ip]
    after = min(g[ip:])
    fell = pk - after > 0.03 * pk
    def reach(x):
        return next((i for i, v in enumerate(g) if v >= x), None)
    def beta_at(i):
        return run.mean("beta", run.t[i] - 0.125, run.t[i] + 0.125) if i is not None else None
    idx = {x: reach(x * pk) for x in (0.2, 0.4, 0.7, 0.8, 0.9, 1.0)}
    i04 = reach(0.4)
    s = lambda i: None if i is None else run.series["steer"][i]  # noqa: E731
    out = dict(peak=pk, peak_t=run.t[ip], fell=fell, steer04=s(i04), steers={f"{x:.1f}": s(reach(x * pk)) for x in FRACTIONS},
               steer09=s(idx[0.9]),
               steer_peak=s(idx[1.0]), beta04=beta_at(i04), beta_peak=beta_at(ip),
               v=run.mean("v", 0, run.t[ip]), gear=run.series["gear"][0])
    if idx[0.2] is not None and idx[0.8] is not None:
        out["dgdt"] = (g[idx[0.8]] - g[idx[0.2]]) / (run.t[idx[0.8]] - run.t[idx[0.2]])
    out["beta_after"] = abs(run.peak("beta", run.t[ip], run.t[ip] + 3.0))
    out["tag"] = "OS" if out["beta_after"] > OVERSTEER else "US"
    out["spun"] = run.spun()
    return out


def analyse_step(run):
    r_ss, g_ss, b_ss = run.mean("r", 3, 4), run.mean("g", 3, 4), run.mean("beta", 3, 4)
    sign = 1.0 if r_ss >= 0 else -1.0
    t90 = run.first(lambda k: sign * run.mean("r", run.t[k] - 0.02, run.t[k] + 0.02) >= 0.9 * abs(r_ss), 0.0, 4.5)
    overshoot = (run.peak("r", 0.0, 4.5, key=lambda x: sign * x) * sign - abs(r_ss)) / abs(r_ss) * 100
    t90g = run.first(lambda k: sign * run.mean("g", run.t[k] - 0.05, run.t[k] + 0.05) >= 0.9 * abs(g_ss), 0.0, 4.5)
    return dict(g=g_ss, r=r_ss, t90=None if t90 is None else t90 - 0.075, overshoot=overshoot, beta=b_ss,
                t90g=None if t90g is None else t90g - 0.075, v=run.mean("v", 3, 4), spun=run.spun())


def analyse_sine(run):
    peak = run.peak("r", 0.0, SINE_END)
    r = lambda t: abs(run.mean("r", t - 0.05, t + 0.05))  # noqa: E731
    return dict(r_peak=abs(peak), r1=r(SINE_END + 1.0) / abs(peak) * 100, r175=r(SINE_END + 1.75) / abs(peak) * 100,
                beta=abs(run.peak("beta", 0.0, 4.0)), spun=run.spun(0.0, 4.0), g=abs(run.peak("g", 0.0, SINE_END)))


def analyse_circle(run):
    n = round(0.5 / run.dt)
    if len(run.before["r"]) < 2 * n:
        return None
    mean = lambda xs: sum(xs[-n:]) / n  # noqa: E731
    r0, b0, v0 = mean(run.before["r"]), mean(run.before["beta"]), mean(run.before["v"])
    early = sum(run.before["r"][:n]) / n
    out = dict(r0=r0, beta0=b0, g0=mean(run.before["g"]), v0=v0)
    # A steady circle: the yaw rate within 10 % of what it was 2 s before, the sideslip under 20 deg, turning.
    out["steady"] = bool(abs(r0 - early) <= 0.1 * abs(r0) and max(abs(x) for x in run.before["beta"]) < 20.0
                         and abs(r0) > 5.0)
    dr = lambda t: run.at("r", t, 0.05) - r0  # noqa: E731
    db = lambda t: run.at("beta", t, 0.05) - b0  # noqa: E731
    ext = lambda f: max((f(0.05 * k) for k in range(1, 81)), key=abs)  # noqa: E731
    out.update(dr1=dr(1.0), db1=db(1.0), dr_peak=ext(dr), db_peak=ext(db), spun=run.spun(0.0, 4.0))
    if abs(r0) > 1.0:
        out["dR1"] = (run.at("v", 1.0) / math.radians(run.at("r", 1.0)) / (v0 / math.radians(r0)) - 1) * 100
        out["ratio1"] = run.at("r", 1.0) / r0
    out["beta1"] = run.at("beta", 1.0)
    out["beta_peak"] = abs(run.peak("beta", 0.0, 4.0))
    return out


def analyse_line(run):
    return dict(decel=-run.mean("gl", 0.3, 1.0), v=run.at("v", 1.0), slip=0.0)


ANALYSE = dict(line=analyse_line, ramp=analyse_ramp, step=analyse_step, sine=analyse_sine, lift=analyse_circle,
               brake=analyse_circle, power=analyse_circle)
KIND = dict(line="brake-in-turn", ramp="ramp-steer", step="step-steer", sine="sine-dwell", lift="lift-in-turn", brake="brake-in-turn",
            power="power-in-turn")


def execute(args, job):
    """Runs a job (name, car, physics, family, kmh, amp, extra) and returns its metrics (from DIR/name.json if present
    and --analyse-only)."""
    name, car, physics, family, kmh, amp, extra = job
    base = os.path.join(args.out, name)
    if not args.analyse_only and not args.fresh and os.path.exists(base + ".json"):
        args = argparse.Namespace(**{**vars(args), "analyse_only": True})
    if not args.analyse_only:
        man = f"{KIND[family]},{kmh:g},{amp:.5f}" + ("" if extra is None else f",{extra:g}")
        cmd = [args.bin, "--autopilot", base + ".out", "--boot", "--stage", args.stage, "--rsf-car", car,
               "--physics", physics, "--steps", "120000", "--flat-pad", str(args.material), "--manoeuvre", man,
               "--trace-speed", base + ".trace", "--data", os.environ["RBR_DATA"],
               "--ngp-physics", os.environ["RBR_NGP_PHYSICS"]]
        with open(base + ".log", "w") as f:
            code = subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT).returncode
        if os.path.exists(base + ".out"):
            os.remove(base + ".out")  # the playback: not needed
        if code or not os.path.exists(base + ".trace"):
            return dict(error=f"exit {code}")
        run = Run(base + ".trace")
        result = ANALYSE[family](run) if run.ok else dict(error="the run never started")
        result = result or dict(error="no run")
        if run.ok and run.extent > PAD_HALF:
            result["error"] = f"left the pad ({run.extent:.0f} m from the start)"
        with open(base + ".json", "w") as f:
            json.dump(result, f)
        if not args.keep_traces:
            os.remove(base + ".trace")
        return result
    if os.path.exists(base + ".json"):
        with open(base + ".json") as f:
            return json.load(f)
    return dict(error="no result")


def run_jobs(args, jobs):
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        return dict(zip((j[0] for j in jobs), pool.map(lambda j: execute(args, j), jobs)))


def fmt(x, spec, none="-"):
    return none if x is None else format(x, spec)


def spread(values, spec):
    values = [v for v in values if v is not None]
    return "-" if not values else f"[{min(values):{spec}}..{max(values):{spec}}]"


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--out", required=True)
    parser.add_argument("--physics", default=PHYSICS)
    parser.add_argument("--cars", default=CARS)
    parser.add_argument("--only", default="ramp,step,sine,lift,brake,power")
    parser.add_argument("--speed", type=float, default=80.0, help="entry speed km/h of all but the sine dwell")
    parser.add_argument("--sine-speeds", default="80,110")
    parser.add_argument("--stage", default="track-85_M")
    parser.add_argument("--material", type=int, default=37, help="the pad's material (37 tarmac fine clean dry)")
    parser.add_argument("--jobs", type=int, default=6)
    parser.add_argument("--analyse-only", action="store_true")
    parser.add_argument("--keep-traces", action="store_true")
    parser.add_argument("--fresh", action="store_true", help="run every case again (else DIR/NAME.json of an earlier run stands)")
    parser.add_argument("--bin", default=BIN, help="the rbr-standalone to run (a variant's build)")
    args = parser.parse_args()
    os.makedirs(args.out, exist_ok=True)
    only = args.only.split(",")
    cases = [(c, p) for c in args.cars.split(",") for p in args.physics.split(",")]
    tag = lambda c, p: f"{short(c)}-{p}"  # noqa: E731

    # Stage 1: the ramp steer, whose steers set the amplitudes of the rest.
    ramp = run_jobs(args, [(f"{tag(c, p)}-ramp-{args.speed:g}", c, p, "ramp", args.speed, RATE, 1.0) for c, p in cases])
    ramps = {(c, p): ramp[f"{tag(c, p)}-ramp-{args.speed:g}"] for c, p in cases}
    for c, p in cases:
        if "error" in ramps[(c, p)] or not ramps[(c, p)].get("steer09"):
            print(f"{tag(c, p)}: no usable ramp ({ramps[(c, p)].get('error', 'no peak')}); the dependent runs are skipped")

    # The straight-line limit of the pedal, per car under the first physics: the lowest pedal within 3 % of the best deceleration.
    pedals = {}
    if "brake" in only:
        first = args.physics.split(",")[0]
        line = run_jobs(args, [(f"{short(c)}-{first}-line-{x:g}", c, first, "line", args.speed, 0.0005, x)
                               for c in args.cars.split(",") for x in LINE_PEDALS])
        for c in args.cars.split(","):
            sweep = [(x, line[f"{short(c)}-{first}-line-{x:g}"]) for x in LINE_PEDALS]
            sweep = [(x, r["decel"]) for x, r in sweep if "error" not in r]
            best = max((d for _, d in sweep), default=0.0)
            limit = next((x for x, d in sweep if d >= 0.97 * best), None)
            print(f"{short(c)}: straight-line braking from {args.speed:g} km/h under {first}: "
                  + " ".join(f"{x:g}:{d:.2f}g" for x, d in sweep) + f"; limit pedal {limit}")
            if limit:
                pedals[c] = {n: round(limit * share, 2) for n, share in LEVELS.items()}

    # The circle manoeuvres' steers: the ramp's steer at the highest share of the peak g (lift and brake 0.9, power 0.8
    # first) whose circle the nominal run holds (steady: the yaw rate unchanged for 2 s, sideslip under 20 deg).
    usable = [(c, p) for c, p in cases if "error" not in ramps[(c, p)] and ramps[(c, p)].get("steer09")]
    start = dict(lift=0.9, brake=0.9, power=0.8)
    chosen = {}

    def probe(case_family):
        (c, p), family = case_family
        r = ramps[(c, p)]
        for f in [x for x in FRACTIONS if x <= start[family] + 1e-9]:
            if r["steers"].get(f"{f:.1f}") is None:
                continue
            kmh = args.speed
            job = (f"{tag(c, p)}-{family}-f{f:.1f}-{kmh:g}", c, p, family if family != "brake" else "lift", kmh,
                   r["steers"][f"{f:.1f}"], None)
            res = execute(args, job)
            if "error" not in res and res.get("steady"):
                return (c, p, family), f
        return (c, p, family), None

    wanted = [(case, fam) for case in usable for fam in ("lift", "power") if fam in only]
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        chosen = dict(pool.map(probe, wanted))
    for key, f in chosen.items():
        print(f"{tag(key[0], key[1])}: highest steady {key[2]} circle: {'none' if f is None else f'{f:.1f}'} of the peak g")
    # Every physics of a car runs the same share: the lowest of those it holds.
    for (c, p, fam) in list(chosen):
        shares = [chosen.get((c, q, fam)) for q in args.physics.split(",") if (c, q) in usable]
        chosen[(c, p, fam)] = None if None in shares else min(shares)
    jobs = []
    for c, p in usable:
        r = ramps[(c, p)]
        t = tag(c, p)
        if "step" in only:
            jobs += [(f"{t}-step-{n}", c, p, "step", args.speed, r[k], None) for n, k in (("low", "steer04"), ("high", "steer09"))]
        for d in DELTAS:
            kmh = args.speed + d
            for fam in ("lift", "power"):
                f = chosen.get((c, p, fam))
                if fam in only and f:
                    jobs.append((f"{t}-{fam}-f{f:.1f}-{kmh:g}", c, p, fam, kmh, r["steers"][f"{f:.1f}"], None))
            f = chosen.get((c, p, "lift"))
            if "brake" in only and f:
                jobs += [(f"{t}-brake-f{f:.1f}-{n}-{kmh:g}", c, p, "brake", kmh, r["steers"][f"{f:.1f}"], v)
                         for n, v in pedals.get(c, {}).items()]
            if "sine" in only:
                for s in args.sine_speeds.split(","):
                    jobs += [(f"{t}-sine-{float(s) + d:g}-{a:g}", c, p, "sine", float(s) + d, a, None) for a in LADDER]
    res = run_jobs(args, jobs)

    def err(r):
        return r is None or "error" in r

    if "ramp" in only:
        print("\nramp steer: peak g, steer (fraction of lock) at 0.4 g and 0.9 peak, turn sideslip (deg) at 0.4 g and the peak, "
              "dg/dt (g/s), limit")
        print(f"{'car':14} {'physics':19} {'km/h':>5} {'gear':>4} {'peak g':>6} {'s@0.4g':>7} {'s@0.9pk':>7} {'b@0.4g':>6} {'b@pk':>6} {'dg/dt':>6} {'b after':>7} limit fell")
        for c, p in cases:
            r = ramps[(c, p)]
            if err(r):
                print(f"{short(c):14} {p:19} {r.get('error')}")
                continue
            print(f"{short(c):14} {p:19} {r['v'] * 3.6:5.1f} {r['gear']:4d} {r['peak']:6.3f} {fmt(r['steer04'], '.3f'):>7} "
                  f"{fmt(r['steer09'], '.3f'):>7} {fmt(r['beta04'], '.1f'):>6} {fmt(r['beta_peak'], '.1f'):>6} "
                  f"{fmt(r.get('dgdt'), '.3f'):>6} {r['beta_after']:7.1f} {r['tag']:>5} {'yes' if r['fell'] else 'no'}")

    if "step" in only:
        print("\nstep steer: steer amplitude, steady lateral g, yaw rate t90 (s), overshoot (%), steady turn sideslip (deg), g t90 (s)")
        print(f"{'car':14} {'physics':19} {'case':>5} {'steer':>6} {'g':>6} {'t90 yaw':>7} {'os %':>6} {'beta':>6} {'t90 g':>6} spun")
        for c, p in cases:
            for n in ("low", "high"):
                r = res.get(f"{tag(c, p)}-step-{n}")
                if err(r):
                    continue
                a = ramps[(c, p)]["steer04" if n == "low" else "steer09"]
                print(f"{short(c):14} {p:19} {n:>5} {a:6.3f} {r['g']:6.3f} {fmt(r['t90'], '.2f'):>7} {r['overshoot']:6.1f} "
                      f"{r['beta']:6.1f} {fmt(r['t90g'], '.2f'):>6} {'yes' if r['spun'] else 'no'}")

    if "sine" in only:
        for s in args.sine_speeds.split(","):
            print(f"\nsine dwell at {s} km/h (+-2 km/h neighbours): steer fraction, peak yaw rate (deg/s) and its g, yaw rate % of the peak "
                  "1.00 s and 1.75 s after the steering is back to zero, peak sideslip (deg), spins of 3")
            print(f"{'car':14} {'physics':19} {'steer':>5} {'g':>5} {'r peak':>6} {'r1.00 %':>7} {'r1.75 %':>7} {'range r1.75':>12} {'beta':>6} spin")
            for c, p in cases:
                first = None
                for a in LADDER:
                    rs = [res.get(f"{tag(c, p)}-sine-{float(s) + d:g}-{a:g}") for d in DELTAS]
                    if err(rs[0]):
                        continue
                    n = sum(1 for r in rs if not err(r) and r["spun"])
                    if rs[0]["spun"] and first is None:
                        first = a
                    r = rs[0]
                    print(f"{short(c):14} {p:19} {a:5.2f} {r['g']:5.2f} {r['r_peak']:6.1f} {r['r1']:7.1f} {r['r175']:7.1f} "
                          f"{spread([x['r175'] for x in rs if not err(x)], '.0f'):>12} {r['beta']:6.1f} {n}/3")
                print(f"{short(c):14} {p:19} lowest spinning amplitude at {s} km/h: {first if first is not None else 'none'}")

    for family, label in (("lift", "lift in turn"), ("power", "power in turn")):
        if family in only:
            print(f"\n{label} at {args.speed:g} km/h (+-2): share of the ramp's peak g the circle is steered for, steady yaw rate r0 (deg/s), g0; "
                  "changes at 1 s and the larger extreme within 4 s (yaw rate deg/s, turn sideslip deg), radius change at 1 s %, "
                  "range of the yaw rate change at 1 s, spins of 3")
            print(f"{'car':14} {'physics':19} {'share':>5} {'r0':>6} {'g0':>5} {'dr1':>7} {'dr pk':>7} {'db1':>6} {'db pk':>6} {'dR1 %':>6} {'range dr1':>14} spin")
            for c, p in cases:
                f = chosen.get((c, p, family))
                if f is None:
                    continue
                rs = [res.get(f"{tag(c, p)}-{family}-f{f:.1f}-{args.speed + d:g}") for d in DELTAS]
                if err(rs[0]):
                    continue
                r = rs[0]
                n = sum(1 for x in rs if not err(x) and x["spun"])
                print(f"{short(c):14} {p:19} {f:5.1f} {r['r0']:6.1f} {r['g0']:5.2f} {r['dr1']:7.1f} {r['dr_peak']:7.1f} {r['db1']:6.1f} "
                      f"{r['db_peak']:6.1f} {fmt(r.get('dR1'), '.1f'):>6} {spread([x['dr1'] for x in rs if not err(x)], '.1f'):>14} {n}/3")

    if "brake" in only:
        print(f"\nbrake in turn at {args.speed:g} km/h (+-2), the lift's circle: pedal, steady yaw rate r0, yaw rate ratio at 1 s, turn sideslip at 1 s and "
              "its change, peak sideslip (deg), range of the ratio, spins of 3")
        print(f"{'car':14} {'physics':19} {'share':>5} {'pedal':>6} {'r0':>6} {'r1/r0':>6} {'beta1':>6} {'db1':>6} {'beta pk':>7} {'range r1/r0':>13} spin")
        for c, p in cases:
            f = chosen.get((c, p, "lift"))
            if f is None:
                continue
            for n, v in pedals.get(c, {}).items():
                rs = [res.get(f"{tag(c, p)}-brake-f{f:.1f}-{n}-{args.speed + d:g}") for d in DELTAS]
                if err(rs[0]):
                    continue
                r = rs[0]
                k = sum(1 for x in rs if not err(x) and x["spun"])
                print(f"{short(c):14} {p:19} {f:5.1f} {v:6.2f} {r['r0']:6.1f} {fmt(r.get('ratio1'), '.2f'):>6} {r['beta1']:6.1f} {r['db1']:6.1f} "
                      f"{r['beta_peak']:7.1f} {spread([x.get('ratio1') for x in rs if not err(x)], '.2f'):>13} {k}/3")


if __name__ == "__main__":
    main()
