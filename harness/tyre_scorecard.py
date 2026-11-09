"""tyre_scorecard.py: score the tyre physics against the published targets in data/tyre-targets.csv.

  python3 -I -B harness/tyre_scorecard.py [--physics ngp,experimental-brush] [--only REGEX] [--quick] [--jobs N]
      [--json OUT] [--work DIR]
  python3 -I -B harness/tyre_scorecard.py --compare A.json B.json

Every target row whose `how` is not `none` is measured for each physics and printed with its band, PASS or MISS and
how far outside the band it lies (bands are inclusive; an empty bound is open). A row with both bounds empty is
context: measured and printed (CTX), not scored. The `how` keys:

  tyre:SURFACE:KEY        a key of the rbr-tyre-lab `scorecard` example (steady forces of the tyre model the physics
                          runs, NGP's where it hands back) on the probed wheel of SURFACE: tarmac (map 85, wheel 1 at
                          step 8000 of the r40 circle), gravel (RSF 345, wheel 0 at step 6000 of the r40 circle), snow
                          (map 10, wheel 0 at step 6000), ice (map 85 iced whole, `--material-as 116`: NGP's
                          material 116 through the tarmac tyre's class factor, as on RSF 565/614; wheel 1 at step
                          8000 of the r40 circle); tarmac-KMH and tarmac-wet-KMH, the tarmac wheel at 32/64/100 km/h,
                          the latter with WaterFactor 1 on the dry friction level; tarmac-wet-80, and
                          tarmac-wet-80-worn / tarmac-wet-100-worn at tread remaining 0.2 (`--tread`),
                          tarmac-wet-80-3mm at 0.375; tarmac-50, tarmac-damp-level-50 and tarmac-wet-level-50, at
                          50 km/h dry and on NGP's damp / wet tarmac level with its WaterFactor; gravel-1.5 and
                          gravel-wet-1.5, the gravel wheel at 1.5 m/s, dry and with WaterFactor 1, on a 25 mm layer
                          (`--soil`); gravel-wet-20, at 20 m/s with WaterFactor 1; gravel-deep and gravel-deeper, the
                          gravel wheel on 17.5 and 25 mm layers
  tyre-ratio:A/B:KEY      the KEY of surface A over surface B's
  brake:SURFACE:KMH:WHAT  brake_test.py's runs from KMH (declutched): locked_g, best_g (shortest threshold stop over
                          the slip targets), ratio (locked decel over best threshold decel), ratio_kK (locked decel
                          over the threshold run at slip K's, one pedal), ratio_absK (locked decel over the per-wheel
                          ABS run at K's, `--brake-abs K`: a run per K the rows name); gravel RSF 345,
                          tarmac RSF 568, snow map 10, ice RSF 504's start straight iced from the brake onset
                          (`--material-as 116`: the run-up on its tarmac; off the road is ice too, so a stop may
                          wander 15 m off the driveline), its decel taken on the flat (less g times the straight's
                          6.6 % rise; a reset by the game, as on RSF 568 where a locked car drifts into the edge, is
                          refused)
  brake-ratio:A/B:KMH:X/Y surface A's X decel (locked, best) over surface B's Y decel
  circle:gravel:lat_g     car_pace.analyse_circle's lateral g on the RSF 345 r40 circle (ramp 0.4 m/s per s; on an
                          11.9° slope, notes/tyre-model.md "Car-level grip in the same terms")
  circle:SURFACE:KEY      car_grip.py's r40 circle on a flat SURFACE (gravel-flat, snow-flat: map 85 under material
                          17 / 101 with the gravel / snow tyre, ramp 0.4; tarmac: map 85 under 37, ramp 0.5): kin_g
                          (v²/R, the circle row's), glat_g (the body-axis specific force over the same 1 s), path_g
  coast:SURFACE:KEY       coast_test.py's declutched coast from 60 km/h to 10 km/h: crr (the rolling coefficient with
                          the car file's drag held), crr_free (the free fit's); tarmac RSF 568, gravel RSF 345, snow
                          map 10, ice RSF 568 iced from the lift (`--material-as 116`: the same road as tarmac)

The car is the Fiesta Rally2 (`--rsf-car Ford_Fiesta_Rally2_ngp7`). --quick keeps the tyre rows and the 50 km/h brake
rows on gravel and tarmac, with three slip targets. --json writes the values per id and physics, the raw measurements,
the commit, the settings and the bands; --compare prints the changes between two such files, each scored on its own
bands (a changed band is flagged). Runs go to --work (default /tmp/tyre-scorecard/<physics>/).
target/release/rbr-standalone and the lab example must be built (`cargo build --release -p rbr-standalone`, `cargo
build --release -p rbr-tyre-lab --example scorecard`); RBR_DATA, RBR_RSF and RBR_NGP_PHYSICS come from dev.env."""
import argparse
import concurrent.futures
import csv
import json
import math
import os
import re
import subprocess
import sys
import types

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import dev_env  # noqa: E402,F401  (dev.env: RBR_DATA, RBR_RSF, RBR_NGP_PHYSICS)
import brake_test  # noqa: E402
import car_grip  # noqa: E402
import car_pace  # noqa: E402
import coast_test  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TARGETS = os.path.join(REPO, "data", "tyre-targets.csv")
BIN = brake_test.BIN
LAB = os.path.join(REPO, "target", "release", "examples", "scorecard")
CAR = "Ford_Fiesta_Rally2_ngp7"
G = brake_test.G
# The rbr_tyre model each physics profile runs (rbr-standalone `profile.rs`, `Profile::tyre`).
TYRE_MODEL = {"ngp": "ngp", "experimental": "ngp", "experimental-brush": "brush"}
# The probe runs: stage, steps, circle, the wheel and step the tyre numbers take.
PROBES = {
    "tarmac": dict(stage=["--stage", "track-85_M"], steps=8000, circle=["--circle", "-260,120,40"], wheel=1, at=8000),
    "gravel": dict(stage=["--rsf-stage", "345"], steps=40000,
                   circle=["--circle", "1051,-166,40", "--circle-ramp", "0.4"], wheel=0, at=6000),
    "snow": dict(stage=["--stage", "track-10_O"], steps=6000, circle=[], wheel=0, at=6000),
    "ice": dict(stage=["--stage", "track-85_M", "--material-as", "116"], steps=8000,
                circle=["--circle", "-260,120,40"], wheel=1, at=8000),
}
TYRE_SURFACES = {"tarmac": ("tarmac", []), "gravel": ("gravel", []), "snow": ("snow", []), "ice": ("ice", [])}
# TRL PPR496's test speeds (32/64/100 km/h) on the tarmac wheel, dry and with WaterFactor 1 (the dry friction level
# kept: the rows are ratios).
for _kmh in (32, 64, 100):
    TYRE_SURFACES[f"tarmac-{_kmh}"] = ("tarmac", ["--speed", f"{_kmh / 3.6:.4f}"])
    TYRE_SURFACES[f"tarmac-wet-{_kmh}"] = ("tarmac", ["--speed", f"{_kmh / 3.6:.4f}", "--water", "1"])
# The wet stops of Contidrom and Continental's 2021 report from 80 and 100 km/h: new, 1.6 mm (tread remaining 0.2), 3 mm.
TYRE_SURFACES["tarmac-wet-80"] = ("tarmac", ["--speed", f"{80 / 3.6:.4f}", "--water", "1"])
for _kmh in (80, 100):
    TYRE_SURFACES[f"tarmac-wet-{_kmh}-worn"] = ("tarmac", ["--speed", f"{_kmh / 3.6:.4f}", "--water", "1", "--tread", "0.2"])
TYRE_SURFACES["tarmac-wet-80-3mm"] = ("tarmac", ["--speed", f"{80 / 3.6:.4f}", "--water", "1", "--tread", "0.375"])
# Damp and wet tarmac at 50 km/h on their own friction level: NGP's tarmac fine clean damp / wet (38 / 39) over dry
# (37), μs 0.925 / 0.85, through the tarmac tyre's class wet slope (1 - 0.075 W), with WaterFactor 0.25 / 1.
TYRE_SURFACES["tarmac-50"] = ("tarmac", ["--speed", f"{50 / 3.6:.4f}"])
for _name, _w, _level in (("damp", 0.25, 0.925), ("wet", 1.0, 0.85)):
    TYRE_SURFACES[f"tarmac-{_name}-level-50"] = ("tarmac", ["--speed", f"{50 / 3.6:.4f}", "--water", str(_w),
                                                            "--friction-scale", f"{_level * (1 - 0.075 * _w):.6f}"])
# Gravel at its sources' speeds: Coutermarsh & Shoop's rig (1-1.5 m/s), dry and with WaterFactor 1 (NGP's wet gravel),
# on NGP's deeper loose layer (25 mm: TR-10-11 gives no depth under the test, hard turns built it up to 10 cm, p. 18),
# and Croft-White's WRC telemetry on wet loose gravel at stage speed (20 m/s), on the probe's layer (RSF 345's 9 mm).
# gravel-deep and gravel-deeper: the probe at NGP's 17.5 and 25 mm layers (loose-v1's α0 follows the depth).
TYRE_SURFACES["gravel-1.5"] = ("gravel", ["--speed", "1.5", "--soil", "0.025"])
TYRE_SURFACES["gravel-wet-1.5"] = ("gravel", ["--speed", "1.5", "--water", "1", "--soil", "0.025"])
TYRE_SURFACES["gravel-wet-20"] = ("gravel", ["--speed", "20", "--water", "1"])
TYRE_SURFACES["gravel-deep"] = ("gravel", ["--soil", "0.0175"])
TYRE_SURFACES["gravel-deeper"] = ("gravel", ["--soil", "0.025"])
# The brake runs per surface: the stage, the extra rbr-standalone flags, the steps, how far off the driveline a valid
# stop may wander, the slip targets (none: the run's) and whether the decel is taken on the flat (grade removed).
BRAKE = {"steps": 30000, "extra": [], "max_off": car_pace.CRASH_OFF, "slips": None, "flat": False}
BRAKE_STAGES = {"gravel": {**BRAKE, "rsf_stage": 345, "stage": None},
                "tarmac": {**BRAKE, "rsf_stage": 568, "stage": None},
                "snow": {**BRAKE, "rsf_stage": None, "stage": "track-10_O"},
                "ice": {**BRAKE, "rsf_stage": 504, "stage": None, "extra": ["--material-as", "116"],
                        "max_off": 15.0, "slips": "0.05,0.1,0.2,0.3", "flat": True}}
# The coast-downs per surface (coast_test.py): the stage, the extra rbr-standalone flags and the off-line limit.
COAST_FROM = 16.67  # m/s: 60 km/h
COAST = {"steps": 80000, "extra": [], "max_off": car_pace.CRASH_OFF}
COAST_STAGES = {"tarmac": {**COAST, "rsf_stage": 568, "stage": None},
                "gravel": {**COAST, "rsf_stage": 345, "stage": None},
                "snow": {**COAST, "rsf_stage": None, "stage": "track-10_O"},
                "ice": {**COAST, "rsf_stage": 568, "stage": None, "extra": ["--material-as", "116"], "max_off": 15.0}}
SLIPS = "0.1,0.15,0.2,0.3,0.5,0.7"
QUICK_SLIPS = "0.2,0.3,0.5"


def load_targets():
    with open(TARGETS, newline="") as f:
        return list(csv.DictReader(f))


def quick_row(row):
    how = row["how"]
    return how.startswith("tyre") or re.fullmatch(r"brake:(gravel|tarmac):50:[\w.]+", how) is not None


def bound(text):
    return float(text) if text.strip() else None


def run(cmd, log):
    """Runs `cmd`, then deletes its autopilot output (hundreds of MB a run, unused here)."""
    with open(log, "w") as f:
        code = subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT).returncode
    out = cmd[cmd.index("--autopilot") + 1]
    if os.path.exists(out):
        os.remove(out)
    return code


class Plan:
    """The runs the selected rows need for one physics, and their results."""

    def __init__(self, physics, hows, work, slips):
        self.physics, self.work, self.slips = physics, work, slips
        self.probes, self.tyres, self.brakes, self.coasts, self.circle = set(), set(), set(), set(), False
        self.circles = set()  # circle:SURFACE:KEY other than RSF 345's gravel probe (car_grip.py's runs)
        self.abs = {}  # (surface, km/h): the per-wheel ABS targets K the rows name
        for how in hows:
            kind, rest = how.split(":", 1)
            if kind == "tyre":
                self.tyres.add(rest.split(":")[0])
            elif kind == "tyre-ratio":
                self.tyres.update(rest.split(":")[0].split("/"))
            elif kind == "brake":
                surface, kmh, what = rest.split(":")
                self.brakes.add((surface, int(kmh)))
                if what.startswith("ratio_abs"):
                    self.abs.setdefault((surface, int(kmh)), set()).add(what[9:])
            elif kind == "brake-ratio":
                pair, kmh, _ = rest.split(":")
                self.brakes.update((s, int(kmh)) for s in pair.split("/"))
            elif kind == "coast":
                self.coasts.add(rest.split(":")[0])
            elif kind == "circle" and rest.split(":")[0] != "gravel":
                self.circles.add(rest.split(":")[0])
            elif kind == "circle":
                self.circle = True
                self.probes.add("gravel")
            else:
                sys.exit(f"unknown how {how}")
        self.probes.update(TYRE_SURFACES[s][0] for s in self.tyres)
        self.tyre, self.brake, self.coast, self.lat_g, self.circle_of = {}, {}, {}, None, {}

    def env_args(self):
        return ["--data", os.environ["RBR_DATA"], "--ngp-physics", os.environ["RBR_NGP_PHYSICS"]]

    def probe_job(self, surface):
        p = PROBES[surface]
        name = os.path.join(self.work, f"probe-{surface}")
        cmd = [BIN, "--autopilot", name + ".out", "--boot", *p["stage"], "--rsf-car", CAR, "--physics", self.physics,
               "--steps", str(p["steps"]), "--top-speed", "60", *p["circle"], "--tyre-probe", name + ".probe",
               "--trace-speed", name + ".trace", *self.env_args()]
        return cmd, name + ".log"

    def brake_jobs(self, surface, kmh):
        frm = round(kmh / 3.6, 2)
        b = BRAKE_STAGES[surface]
        args = types.SimpleNamespace(rsf_car=CAR, physics=self.physics, steps=b["steps"], top_speed=80.0,
                                     clutch_out=False, out=self.work, extra=b["extra"], rsf_stage=b["rsf_stage"],
                                     stage=b["stage"], **{"from": frm})
        runs = brake_test.runs_for(self.slips_of(surface), self.abs_of(surface, kmh))
        for name, brake in runs:
            full = f"brake-{surface}-{kmh}-{name}"
            yield brake_test.command(args, full, brake), os.path.join(self.work, full + ".log")

    def coast_job(self, surface):
        c = COAST_STAGES[surface]
        args = types.SimpleNamespace(rsf_car=CAR, physics=self.physics, steps=c["steps"], top_speed=80.0,
                                     clutch_out=False, out=self.work, extra=c["extra"], rsf_stage=c["rsf_stage"],
                                     stage=c["stage"], **{"from": COAST_FROM})
        name = f"coast-{surface}"
        return coast_test.command(args, name), os.path.join(self.work, name + ".log")

    def read_coasts(self):
        for surface in sorted(self.coasts):
            trace = os.path.join(self.work, f"coast-{surface}.trace")
            self.coast[surface] = (coast_test.analyse(trace, COAST_FROM, CAR, max_off=COAST_STAGES[surface]["max_off"])
                                   if os.path.exists(trace) else {"ok": False, "reason": "no trace"})

    def slips_of(self, surface):
        return BRAKE_STAGES[surface]["slips"] or self.slips

    def abs_of(self, surface, kmh):
        return ",".join(sorted(self.abs.get((surface, kmh), ())))

    def jobs(self):
        out = [self.probe_job(s) for s in sorted(self.probes)]
        for surface, kmh in sorted(self.brakes):
            out += list(self.brake_jobs(surface, kmh))
        out += [self.coast_job(s) for s in sorted(self.coasts)]
        return out + [self.circle_job(s) for s in sorted(self.circles)]

    def circle_job(self, surface):
        trace = os.path.join(self.work, f"circle-{surface}.trace")
        cmd = car_grip.command(CAR, self.physics, surface, 40, car_grip.own_ramp(surface), trace)
        cmd[cmd.index("--autopilot") + 1] = trace[:-6] + ".out"  # run() deletes it
        return cmd, trace[:-6] + ".log"

    def read_circles(self):
        for surface in sorted(self.circles):
            trace = os.path.join(self.work, f"circle-{surface}.trace")
            cx, cy, r = car_grip.limit_pace.SURFACES[surface]["centre"]
            self.circle_of[surface] = (car_grip.analyse(trace, cx, cy, r) if os.path.exists(trace)
                                       else {"ok": False, "why": "no trace"})

    def tyre_job(self, surface):
        probe, extra = TYRE_SURFACES[surface]
        p = PROBES[probe]
        return [LAB, "--probe", os.path.join(self.work, f"probe-{probe}.probe"), "--step", str(p["at"]),
                "--wheel", str(p["wheel"]), "--model", TYRE_MODEL[self.physics], *extra]

    def read_tyre(self, surface):
        cmd = self.tyre_job(surface)
        done = subprocess.run(cmd, capture_output=True, text=True)
        if done.returncode != 0:
            self.tyre[surface] = {"error": done.stderr.strip().splitlines()[-1:]}
            return
        values = {}
        for line in done.stdout.splitlines():
            if line.startswith("#"):
                values["#"] = line[2:]
            elif line.strip():
                key, value = line.split()
                values[key] = float(value)
        self.tyre[surface] = values

    def read_brakes(self):
        for surface, kmh in sorted(self.brakes):
            frm = round(kmh / 3.6, 2)
            runs = {}
            b = BRAKE_STAGES[surface]
            for name, _ in brake_test.runs_for(self.slips_of(surface), self.abs_of(surface, kmh)):
                trace = os.path.join(self.work, f"brake-{surface}-{kmh}-{name}.trace")
                target = float(name[3:]) if name.startswith("abs") else None
                result = (brake_test.analyse(trace, frm, target, max_off=b["max_off"]) if os.path.exists(trace)
                          else {"ok": False, "reason": "no trace"})
                decel = "decel_flat" if b["flat"] else "decel"
                runs[name] = ({"dist": result["dist"], "time": result["time"], "g": result[decel] / G,
                               "grade": result["grade"]} if result["ok"] else {"error": result["reason"]})
                if result.get("wheels"):
                    runs[name]["wheels"] = result["wheels"]  # each wheel's mean slip and ABS cycle rate (Hz)
            ok = {k: v for k, v in runs.items() if k.startswith("k") and "g" in v}
            best = min(ok, key=lambda k: ok[k]["dist"]) if ok else None
            self.brake[(surface, kmh)] = {"runs": runs, "best": best}

    def read_circle(self):
        trace = os.path.join(self.work, "probe-gravel.trace")
        if not os.path.exists(trace):
            self.lat_g = {"error": "no trace"}
            return
        r = car_pace.analyse_circle(car_pace.load_trace(trace), car_pace.GCIRC_CX, car_pace.GCIRC_CY,
                                    car_pace.GCIRC_R)
        self.lat_g = ({"lat_g": r["latg"], "vmax_kmh": r["vmax"], "slid": r["slid"]} if r["ok"]
                      else {"error": r["reason"]})

    def brake_g(self, surface, kmh, what):
        b = self.brake[(surface, kmh)]
        name = {"locked": "locked", "best": b["best"]}.get(what, what)
        run_ = b["runs"].get(name) if name else None
        if not run_ or "g" not in run_:
            raise LookupError(f"{surface} {kmh} km/h {what}: " + (run_ or {}).get("error", "no threshold run"))
        return run_["g"]

    def value(self, how):
        """The measured value of `how`, or raise LookupError with the reason."""
        kind, rest = how.split(":", 1)
        if kind == "tyre":
            surface, key = rest.split(":")
            t = self.tyre[surface]
            if key not in t:
                raise LookupError(t.get("error") or f"no key {key}")
            return t[key]
        if kind == "tyre-ratio":
            pair, key = rest.split(":")
            a, b = pair.split("/")
            return self.value(f"tyre:{a}:{key}") / self.value(f"tyre:{b}:{key}")
        if kind == "brake":
            surface, kmh, what = rest.split(":")
            if what == "ratio":
                return self.brake_g(surface, int(kmh), "locked") / self.brake_g(surface, int(kmh), "best")
            if what.startswith(("ratio_k", "ratio_abs")):
                return self.brake_g(surface, int(kmh), "locked") / self.brake_g(surface, int(kmh), what[6:])
            return self.brake_g(surface, int(kmh), what.removesuffix("_g"))
        if kind == "brake-ratio":
            pair, kmh, xy = rest.split(":")
            (a, b), (x, y) = pair.split("/"), xy.split("/")
            return self.brake_g(a, int(kmh), x) / self.brake_g(b, int(kmh), y)
        if kind == "coast":
            surface, key = rest.split(":")
            c = self.coast[surface]
            if not c["ok"]:
                raise LookupError(c["reason"])
            return c[key]
        if kind == "circle" and rest.split(":")[0] != "gravel":
            surface, key = rest.split(":")
            c = self.circle_of[surface]
            if not c["ok"]:
                raise LookupError(c["why"])
            return c[key]
        if kind == "circle":
            if "lat_g" not in self.lat_g:
                raise LookupError(self.lat_g["error"])
            return self.lat_g["lat_g"]
        raise LookupError(f"unknown how {how}")

    def raw(self):
        return {"tyre": self.tyre, "circle": self.lat_g, "circles": self.circle_of, "coast": self.coast,
                "brake": {f"{s}:{k}": v for (s, k), v in self.brake.items()}}


def verdict(value, low, high):
    """PASS or MISS and the signed distance outside the band (0 inside); CTX for a context row (no band)."""
    if low is None and high is None:
        return "CTX", 0.0
    if math.isnan(value):  # not reached (e.g. an aligning moment that never changes sign)
        return "MISS", value
    if low is not None and value < low:
        return "MISS", value - low
    if high is not None and value > high:
        return "MISS", value - high
    return "PASS", 0.0


def band_text(low, high):
    return f"{'' if low is None else f'{low:g}'}..{'' if high is None else f'{high:g}'}"


def commit():
    head = subprocess.run(["git", "-C", REPO, "rev-parse", "--short", "HEAD"], capture_output=True, text=True)
    dirty = subprocess.run(["git", "-C", REPO, "status", "--porcelain", "--untracked-files=no"], capture_output=True,
                           text=True)
    return head.stdout.strip() + ("-dirty" if dirty.stdout.strip() else "")


def score(args):
    for path, build in ((BIN, "cargo build --release -p rbr-standalone"),
                        (LAB, "cargo build --release -p rbr-tyre-lab --example scorecard")):
        if not os.path.exists(path):
            sys.exit(f"{path}: build it first ({build})")
    rows = [r for r in load_targets() if r["how"] != "none"]
    if args.only:
        rows = [r for r in rows if re.search(args.only, r["id"]) or re.search(args.only, r["how"])]
    if args.quick:
        rows = [r for r in rows if quick_row(r)]
    if not rows:
        sys.exit("no rows selected")
    physics = args.physics.split(",")
    for p in physics:
        if p not in TYRE_MODEL:
            sys.exit(f"--physics {p}: one of {', '.join(TYRE_MODEL)}")
    slips = QUICK_SLIPS if args.quick else args.slips
    plans = {}
    for p in physics:
        work = os.path.join(args.work, p)
        os.makedirs(work, exist_ok=True)
        plans[p] = Plan(p, {r["how"] for r in rows}, work, slips)
    jobs = [job for plan in plans.values() for job in plan.jobs()]
    print(f"{len(rows)} rows, {len(jobs)} runs, {args.jobs} jobs", file=sys.stderr)
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        list(pool.map(lambda j: run(*j), jobs))
    for plan in plans.values():
        for surface in sorted(plan.tyres):
            plan.read_tyre(surface)
        plan.read_brakes()
        plan.read_coasts()
        if plan.circle:
            plan.read_circle()
        plan.read_circles()

    values = {}
    print(f"{'id':40} {'physics':18} {'value':>8} {'band':>13} {'':4} {'out':>8}")
    for row in rows:
        low, high = bound(row["low"]), bound(row["high"])
        values[row["id"]] = {}
        for p, plan in plans.items():
            try:
                value = plan.value(row["how"])
            except (LookupError, ZeroDivisionError) as e:
                print(f"{row['id']:40} {p:18} {'n/a':>8} {band_text(low, high):>13} {'':4} {e}")
                values[row["id"]][p] = None
                continue
            status, out = verdict(value, low, high)
            print(f"{row['id']:40} {p:18} {value:8.3f} {band_text(low, high):>13} {status:4} "
                  f"{'' if status != 'MISS' else f'{out:+8.3f}':>8}")
            values[row["id"]][p] = value
    scored = [r for r in rows if bound(r["low"]) is not None or bound(r["high"]) is not None]
    counts = {p: sum(1 for r in scored if values[r["id"]][p] is not None and
                     verdict(values[r["id"]][p], bound(r["low"]), bound(r["high"]))[0] == "PASS") for p in plans}
    for p in plans:
        print(f"{p}: {counts[p]}/{len(scored)} PASS")
    if args.json:
        out = {"commit": commit(),
               "settings": {"physics": physics, "car": CAR, "quick": args.quick, "only": args.only, "slips": slips,
                            "probes": PROBES, "targets": os.path.relpath(TARGETS, REPO)},
               "bands": {r["id"]: [bound(r["low"]), bound(r["high"])] for r in rows},
               "values": values, "raw": {p: plan.raw() for p, plan in plans.items()}}
        os.makedirs(os.path.dirname(os.path.abspath(args.json)), exist_ok=True)
        with open(args.json, "w") as f:
            json.dump(out, f, indent=1, sort_keys=True)


def compare(a_path, b_path):
    """Each file's values against its own stored bands (the targets file's for a file without them); a row whose band
    changed is printed with both bands and `band` in its change."""
    with open(a_path) as f:
        a = json.load(f)
    with open(b_path) as f:
        b = json.load(f)
    current = {r["id"]: (bound(r["low"]), bound(r["high"])) for r in load_targets()}

    def bands(x):
        stored = x.get("bands")
        return {rid: tuple(v) for rid, v in stored.items()} if stored is not None else current

    bands_a, bands_b = bands(a), bands(b)
    print(f"A {a_path} ({a['commit']})\nB {b_path} ({b['commit']})")
    print(f"{'id':40} {'physics':18} {'A':>8} {'B':>8} {'B-A':>8} {'band':>13}  change")
    only = sorted(set(a["values"]) ^ set(b["values"]))
    if only:
        print(f"measured in one file only (skipped): {', '.join(only)}")
    for rid in sorted(set(a["values"]) & set(b["values"])):
        band_a, band_b = bands_a.get(rid, (None, None)), bands_b.get(rid, (None, None))
        moved = band_a != band_b
        for p in sorted(set(a["values"][rid]) & set(b["values"][rid])):
            va, vb = a["values"][rid].get(p), b["values"][rid].get(p)
            sa = verdict(va, *band_a)[0] if va is not None else "n/a"
            sb = verdict(vb, *band_b)[0] if vb is not None else "n/a"
            close = va is not None and vb is not None and (abs(vb - va) < 5e-4 or math.isnan(va) and math.isnan(vb))
            if close and sa == sb and not moved:
                continue
            fa = f"{va:8.3f}" if va is not None else f"{'n/a':>8}"
            fb = f"{vb:8.3f}" if vb is not None else f"{'n/a':>8}"
            fd = f"{vb - va:+8.3f}" if va is not None and vb is not None else f"{'':>8}"
            change = f"{sa} -> {sb}" if sa != sb else sb
            band = band_text(*band_b)
            if moved:
                band, change = f"{band_text(*band_a)} -> {band}", f"{change} (band)"
            print(f"{rid:40} {p:18} {fa} {fb} {fd} {band:>13}  {change}")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--physics", default="ngp,experimental-brush")
    parser.add_argument("--only", help="regex over the ids and hows")
    parser.add_argument("--quick", action="store_true")
    parser.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 2) // 2))
    parser.add_argument("--slips", default=SLIPS, help="threshold braking's slip targets K")
    parser.add_argument("--work", default="/tmp/tyre-scorecard")
    parser.add_argument("--json")
    parser.add_argument("--compare", nargs=2, metavar=("A", "B"))
    args = parser.parse_args()
    if args.compare:
        compare(*args.compare)
    else:
        score(args)


if __name__ == "__main__":
    main()
