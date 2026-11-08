"""limit_pace.py: the autopilot at each car's own limits on a stage, for the stage statistics against real drives.

  python3 -I -B harness/limit_pace.py limits --out DIR [--cars A,B] [--physics ngp,experimental-brush] [--jobs N]
  python3 -I -B harness/limit_pace.py search --out DIR [--rsf-stage 524] [--factors 0.8,0.9,1.0,1.1]
      [--top-speed 60] [--steps N] [--max-off M] [--cars A,B] [--physics P,Q] [--jobs N]

`limits` measures each car × physics on RSF 345 (gravel, `tyre_scorecard.py`'s gravel stage): the r40 circle's
lateral g (`--circle 1051,-166,40 --circle-ramp 0.4`, car_pace.analyse_circle, the scorecard's circle key) and the
best threshold braking g from 80 km/h (brake_test.py's declutched runs, slip targets SLIPS: the shortest clean stop's
mean decel, the road's grade in). It writes DIR/limits.json.

`search` drives the stage once per factor F with the autopilot's speed plan at F times the car's limits
(`--plan LAT,BRAKE` = F x lateral g x G, F x braking g x G, m/s²; with `--brake-factor B`, B in place of F on the
braking, for a car whose circle reads off) and `--top-speed` (m/s, 60 by default: above
any car's top here, so the plan and the car limit the speed, not the cap), with `--trace-speed`, to
DIR/<car>.<physics>.f<F>.trace (log .log). A run is clean when the car finishes, is not retired, the log has no
recovery ("backing up"), outside assistance or end other than the finish, and the trace's distance from the
driveline (`off`, the controller's tracking error) stays under MAX_OFF m all the way (the default runs keep under
1 m; a car further out than MAX_OFF has run wide of the line, typically off the road's edge on a 6-8 m gravel road).
The fastest clean run per car × physics is the chosen plan; DIR/search.csv lists every run in DIR of the cars and physics given (earlier calls' too), DIR/chosen.txt the
chosen traces (feed them to `rally_gg.py stats`). Runs with a log that already ends are not run again.
target/release/rbr-standalone must be built; RBR_DATA, RBR_RSF and RBR_NGP_PHYSICS come from dev.env.
"""
import argparse
import concurrent.futures
import csv
import glob
import json
import os
import re
import subprocess
import sys
import types

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import dev_env  # noqa: E402,F401  (dev.env: RBR_DATA, RBR_RSF, RBR_NGP_PHYSICS)
import brake_test  # noqa: E402
import car_pace  # noqa: E402
import rally_gg  # noqa: E402

BIN = brake_test.BIN
G = brake_test.G
CIRCLE = ["--circle", f"{car_pace.GCIRC_CX:.0f},{car_pace.GCIRC_CY:.0f},{car_pace.GCIRC_R:.0f}", "--circle-ramp", "0.4"]
LIMIT_STAGE = 345
BRAKE_FROM = 22.22  # m/s: 80 km/h
SLIPS = "0.1,0.15,0.2,0.3,0.5"
MAX_OFF = 3.0  # m


def env_args():
    return ["--data", os.environ["RBR_DATA"], "--ngp-physics", os.environ["RBR_NGP_PHYSICS"]]


def run(job):
    cmd, log = job
    with open(log, "w") as f:
        code = subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT).returncode
    out = cmd[cmd.index("--autopilot") + 1]
    if out.endswith(".out") and os.path.exists(out):
        os.remove(out)  # the playback dump (0.1-5 GB): only the trace is read
    return code


def pairs(args):
    return [(car, physics) for car in args.cars.split(",") for physics in args.physics.split(",")]


def limits(args):
    os.makedirs(args.out, exist_ok=True)
    jobs = []
    for car, physics in pairs(args):
        name = os.path.join(args.out, f"circle.{car}.{physics}")
        jobs.append(([BIN, "--autopilot", "/dev/null", "--boot", "--rsf-stage", str(LIMIT_STAGE), "--rsf-car", car,
                      "--physics", physics, "--steps", "40000", "--top-speed", "60", *CIRCLE,
                      "--trace-speed", name + ".trace", *env_args()], name + ".log"))
        work = os.path.join(args.out, f"brake.{car}.{physics}")
        os.makedirs(work, exist_ok=True)
        b = types.SimpleNamespace(rsf_car=car, physics=physics, steps=30000, top_speed=80.0, clutch_out=False,
                                  out=work, extra=[], rsf_stage=LIMIT_STAGE, stage=None, **{"from": BRAKE_FROM})
        for run_name, brake in brake_test.runs_for(SLIPS, ""):
            jobs.append((brake_test.command(b, run_name, brake), os.path.join(work, run_name + ".log")))
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        list(pool.map(run, jobs))
    out = {}
    for car, physics in pairs(args):
        r = car_pace.analyse_circle(car_pace.load_trace(os.path.join(args.out, f"circle.{car}.{physics}.trace")),
                                    car_pace.GCIRC_CX, car_pace.GCIRC_CY, car_pace.GCIRC_R)
        work = os.path.join(args.out, f"brake.{car}.{physics}")
        stops = {}
        for run_name, _ in brake_test.runs_for(SLIPS, ""):
            trace = os.path.join(work, run_name + ".trace")
            s = brake_test.analyse(trace, BRAKE_FROM) if os.path.exists(trace) else {"ok": False}
            if s["ok"]:
                stops[run_name] = s
        best = min((k for k in stops if k != "locked"), key=lambda k: stops[k]["dist"], default=None)
        out[f"{car}.{physics}"] = {
            "lat_g": r["latg"] if r["ok"] else None, "circle_vmax_kmh": r.get("vmax"),
            "brake_g": stops[best]["decel"] / G if best else None, "brake_run": best,
            "locked_g": stops["locked"]["decel"] / G if "locked" in stops else None,
        }
        print(f"{car} {physics}: circle {out[f'{car}.{physics}']['lat_g']} g, best threshold {best} "
              f"{out[f'{car}.{physics}']['brake_g']} g, locked {out[f'{car}.{physics}']['locked_g']} g")
    with open(os.path.join(args.out, "limits.json"), "w") as f:
        json.dump(out, f, indent=1)


def max_off(trace):
    col = None
    worst = 0.0
    with open(trace) as f:
        col = f.readline().split()[1:].index("off")
        for line in f:
            v = line.split()
            if len(v) > col:
                worst = max(worst, float(v[col]))
    return worst


def judge(name, max_off_limit):
    """(clean, time s or None, why, worst off m) of the run `name` (its .log and .trace)."""
    log = open(name + ".log").read()
    m = re.search(r"end_race: EndRace \{ time: ([0-9.]+).*retired: (true|false)", log)
    time = float(m.group(1)) if m else None
    worst = max_off(name + ".trace") if os.path.exists(name + ".trace") else float("nan")
    why = []
    if "steps after the finish" not in log:
        why.append("no finish: " + (re.findall(r"ended by (.*)", log) or ["?"])[-1])
    if m and m.group(2) == "true":
        why.append("retired")
    if "backing up" in log:
        why.append(f"{log.count('backing up')} recoveries")
    if "outside assistance" in log.lower():
        why.append("outside assistance")
    if not worst <= max_off_limit:
        why.append(f"off {worst:.1f} m")
    return not why, time, "; ".join(why), worst


def search(args):
    lim = json.load(open(os.path.join(args.out, "limits.json")))
    jobs = []
    for car, physics in pairs(args):
        lg, bg = lim[f"{car}.{physics}"]["lat_g"], lim[f"{car}.{physics}"]["brake_g"]
        for f in args.factors.split(","):
            fb = args.brake_factor or f
            name = os.path.join(args.out, f"{car}.{physics}.f{f}" + (f"b{fb}" if args.brake_factor else ""))
            plan = f"{float(f) * lg * G:.3f},{float(fb) * bg * G:.3f}"
            if os.path.exists(name + ".log") and "ended by" in open(name + ".log").read():
                continue
            jobs.append(([BIN, "--autopilot", "/dev/null", "--boot", "--rsf-stage", str(args.rsf_stage),
                          "--rsf-car", car, "--physics", physics, "--steps", str(args.steps),
                          "--top-speed", str(args.top_speed), "--plan", plan, "--trace-speed", name + ".trace",
                          *env_args()], name + ".log"))
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        list(pool.map(run, jobs))
    # Every run in DIR of these cars and physics, this call's and earlier ones'.
    done = []
    for car, physics in pairs(args):
        for log in sorted(glob.glob(os.path.join(glob.escape(args.out), f"{car}.{physics}.f*.log"))):
            name = log[:-4]
            plan = re.search(r"lateral ([0-9.]+) m/s², braking ([0-9.]+) m/s² \(--plan\)", open(log).read())
            done.append((car, physics, name.rsplit(".f", 1)[1], f"{plan.group(1)},{plan.group(2)}" if plan else "?", name))
    rows, best = [], {}
    for car, physics, f, plan, name in done:
        clean, time, why, worst = judge(name, args.max_off)
        rows.append([car, physics, f, plan, f"{time:.2f}" if time else "", clean, f"{worst:.2f}", why])
        key = (car, physics)
        if clean and (key not in best or time < best[key][0]):
            best[key] = (time, f, plan, name)
        print(f"{car} {physics} f{f} plan {plan}: {time and f'{time:.2f} s'} off max {worst:.2f} m "
              f"{'clean' if clean else why}")
    with open(os.path.join(args.out, "search.csv"), "w", newline="") as h:
        w = csv.writer(h)
        w.writerow(["car", "physics", "factor", "plan", "time_s", "clean", "max_off_m", "why"])
        w.writerows(rows)
    with open(os.path.join(args.out, "chosen.txt"), "w") as h:
        for (car, physics), (time, f, plan, name) in sorted(best.items()):
            print(f"chosen {car} {physics}: factor {f}, plan {plan} m/s², {time:.2f} s")
            h.write(f"{name}.trace {f} {plan} {time:.2f}\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="mode", required=True)
    for mode in ("limits", "search"):
        p = sub.add_parser(mode)
        p.add_argument("--out", required=True)
        p.add_argument("--cars", default=rally_gg.CARS)
        p.add_argument("--physics", default="ngp,experimental-brush")
        p.add_argument("--jobs", type=int, default=6)
    s = sub.choices["search"]
    s.add_argument("--rsf-stage", type=int, default=524)
    s.add_argument("--factors", default="0.8,0.9,1.0,1.1")
    s.add_argument("--top-speed", type=float, default=60.0)
    s.add_argument("--steps", type=int, default=500000)
    s.add_argument("--max-off", type=float, default=MAX_OFF)
    s.add_argument("--brake-factor", help="hold the braking factor here (the factors then scale the lateral only)")
    args = parser.parse_args()
    if not os.path.exists(BIN):
        sys.exit(f"{BIN}: build it first (cargo build --release -p rbr-standalone)")
    (limits if args.mode == "limits" else search)(args)


if __name__ == "__main__":
    main()
