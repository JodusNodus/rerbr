"""coast_test.py: a declutched coast-down, one car on one stage, through the Rust autopilot: its rolling resistance.

  python3 -I -B harness/coast_test.py --rsf-stage 568 --rsf-car Ford_Fiesta_Rally2_ngp7 --from 16.67 --out DIR
      [--physics ngp|experimental|experimental-brush] [--to 2.78] [--drag B] [--steps N] [--stage NAME]
      [-- EXTRA rbr-standalone ARGS]

The run is brake_test.py's with a pedal that quantizes to 0 (`--brake-pedal 0.001`: the u8 pedal rounds to 0): the
autopilot drives the stage flat out and, once the speed is reached, lifts off, holds the clutch in and shifts nothing
(`--brake-declutch --clutch-help off`) and steers on, so the car coasts. Put `--stage pad-<name>` after `--` to run
the whole coast on that pad material (as for the brakes).

The analysis cuts the coast, from SETTLE s past the lift to `--to` (m/s, 10 km/h unless given), into SEG metre
pieces of the path and takes each piece's resistance from its energy, -(Δ(v²/2) + g Δz) / Δs (m/s²; the grade
removed, the car's z with its pitch), keeping the pieces whose lateral acceleration (from the heading's change) is
under LAT_MAX (a corner's slip drag left out). Less the drag b v² (b the car file's, `car_drag`, or `--drag B`), the
pieces' median over g is the car's rolling coefficient crr: the tyres, the soil, the wheels' bearings and the
declutched drive line, the downforce's load aside. It also prints the free fit res = a + b v² (a / g and b; a check
of the drag on a coast from 100 km/h). The coast stops at a reset by the game, past `--max-off` off the driveline or
at the run's end. target/release/rbr-standalone must be built; RBR_DATA, RBR_RSF and RBR_NGP_PHYSICS come from
dev.env."""
import argparse
import math
import os
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import dev_env  # noqa: E402,F401  (dev.env: RBR_DATA, RBR_RSF, RBR_NGP_PHYSICS)
import brake_test  # noqa: E402
import car_pace  # noqa: E402

G = brake_test.G
COAST = ["--brake-pedal", "0.001"]  # rounds to a u8 pedal of 0: no brake, the brake test's declutch and steering
SETTLE = 1.5  # s past the lift before the pieces start (the clutch, the engine braking and the pitch settle)
SEG = 10.0  # m of path a piece
LAT_MAX = 0.5  # m/s²: pieces turning harder are a corner, left out
OUTLIER = 0.004  # g off the held line: a piece over a bump or a dip the free fit leaves out
TO = 10 / 3.6  # m/s: the coast's end unless --to


def command(args, name):
    """The coast run `name`: brake_test's command with the zero pedal."""
    return brake_test.command(args, name, COAST)


def pieces(rows, frm, to=TO, max_off=car_pace.CRASH_OFF):
    """The coast's pieces (v m/s, res m/s², lateral m/s², rise m) from the lift to `to`, or a reason string."""
    launch = car_pace.find_launch([r["v"] for r in rows])
    if launch is None:
        return "no launch"
    trig = next((i for i in range(launch, len(rows)) if rows[i]["v"] >= frm), None)
    if trig is None:
        return f"never reached {frm * 3.6:.0f} km/h"
    lift = next((i for i in range(trig, len(rows)) if rows[i]["thr"] < 0.01), None)
    if lift is None:
        return "never lifted"
    start = next((i for i in range(lift, len(rows)) if rows[i]["t"] - rows[lift]["t"] >= SETTLE), len(rows))
    out, i = [], start
    while i < len(rows) - 1 and rows[i]["v"] > to:
        s, j = 0.0, i
        while j < len(rows) - 1 and s < SEG:
            a, b = rows[j], rows[j + 1]
            step = math.dist((a["x"], a["y"], a["z"]), (b["x"], b["y"], b["z"]))
            if step > car_pace.RESET_JUMP or abs(b["off"]) > max_off:
                return out
            s, j = s + step, j + 1
        if s < SEG:
            break
        a, m, b = rows[i], rows[(i + j) // 2], rows[j]
        turn = math.atan2(b["y"] - m["y"], b["x"] - m["x"]) - math.atan2(m["y"] - a["y"], m["x"] - a["x"])
        turn = (turn + math.pi) % (2 * math.pi) - math.pi
        v = s / (b["t"] - a["t"])
        out.append({"v": v, "res": -(0.5 * (b["v"] ** 2 - a["v"] ** 2) + G * (b["z"] - a["z"])) / s,
                    "lat": v * v * turn / (s / 2), "rise": b["z"] - a["z"]})
        i = j
    return out


def car_drag(car, z=0.0):
    """The car's drag per mass b = rho Cd A / (2 m) (1/m): FrontArea, DragCoefficient and InverseMass of the car's
    common.lsp (RBR_RSF's rsfdata, read at run time), rho NGP's atmosphere at height z (notes/ngp.md: T = 293.15 -
    0.0065 z, p = 101300 (1 - 0.0065 z / 288.15)^5.255, rho = p / 287 T). The mass is the body's (the wheels'
    bodies left out)."""
    path = os.path.join(os.environ["RBR_RSF"], "rsfdata", "cars", car, "common.lsp")
    with open(path, errors="replace") as f:
        text = f.read()
    value = {k: float(re.search(rf"\b{k}\s+([-0-9.eE]+)", text).group(1))
             for k in ("FrontArea", "DragCoefficient", "InverseMass")}
    rho = 101300 * (1 - 0.0065 * z / 288.15) ** 5.255 / (287 * (293.15 - 0.0065 * z))
    return 0.5 * rho * value["FrontArea"] * value["DragCoefficient"] * value["InverseMass"]


def line(x, y):
    mx, my = sum(x) / len(x), sum(y) / len(y)
    b = sum((xi - mx) * (yi - my) for xi, yi in zip(x, y)) / sum((xi - mx) ** 2 for xi in x)
    return my - b * mx, b


def fit(pieces_, drag):
    """Over the straight pieces, res - drag v² = a: crr = a / g with the drag held (the median, so a bump's piece
    does not count), its pieces' spread (g, the interquartile range over 1.35), and the free fit res = a + b v²
    over the pieces within OUTLIER g of the held line (crr_free, b_free)."""
    kept = [p for p in pieces_ if abs(p["lat"]) < LAT_MAX]
    if len(kept) < 3:
        return {"ok": False, "reason": f"{len(kept)} straight pieces"}
    rest = sorted((p["res"] - drag * p["v"] ** 2) / G for p in kept)
    q = lambda f: rest[min(len(rest) - 1, int(f * len(rest)))]
    crr = q(0.5)
    clean = [p for p in kept if abs((p["res"] - drag * p["v"] ** 2) / G - crr) < OUTLIER]
    a, b = line([p["v"] ** 2 for p in clean], [p["res"] for p in clean]) if len(clean) >= 3 else (float("nan"),) * 2
    return {"ok": True, "crr": crr, "spread": (q(0.75) - q(0.25)) / 1.35, "drag": drag, "crr_free": a / G,
            "b_free": b, "n": len(kept), "of": len(pieces_), "outliers": len(kept) - len(clean),
            "v": (min(p["v"] for p in kept), max(p["v"] for p in kept)),
            "grade": sum(p["rise"] for p in kept) / (SEG * len(kept))}


def analyse(path, frm, car, to=TO, max_off=car_pace.CRASH_OFF, drag=None):
    """The coast of trace `path`: fit()'s values, the drag held at `drag` or the car's at the coast's mean height."""
    rows = car_pace.load_trace(path)
    got = pieces(rows, frm, to, max_off)
    if isinstance(got, str):
        return {"ok": False, "reason": got}
    if drag is None:
        drag = car_drag(car, sum(r["z"] for r in rows) / len(rows))
    return fit(got, drag)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    stage = parser.add_mutually_exclusive_group(required=True)
    stage.add_argument("--rsf-stage", type=int)
    stage.add_argument("--stage")
    parser.add_argument("--rsf-car", required=True)
    parser.add_argument("--physics", default="ngp")
    parser.add_argument("--from", type=float, required=True, help="the lift's speed, m/s")
    parser.add_argument("--to", type=float, default=TO, help="the coast's end, m/s")
    parser.add_argument("--drag", type=float, help="the drag per mass b held (1/m; the car file's unless given)")
    parser.add_argument("--max-off", type=float, default=car_pace.CRASH_OFF)
    parser.add_argument("--steps", type=int, default=80000)
    parser.add_argument("--top-speed", type=float, default=80.0)
    parser.add_argument("--out", required=True)
    parser.add_argument("extra", nargs="*", help="after --: more rbr-standalone arguments")
    args = parser.parse_args()
    args.clutch_out = False
    if not os.path.exists(brake_test.BIN):
        sys.exit(f"{brake_test.BIN}: build it first (cargo build --release -p rbr-standalone)")
    os.makedirs(args.out, exist_ok=True)
    cmd = command(args, "coast")
    with open(os.path.join(args.out, "coast.log"), "w") as f:
        subprocess.run(cmd, stdout=f, stderr=subprocess.STDOUT)
    out = cmd[cmd.index("--autopilot") + 1]
    if os.path.exists(out):
        os.remove(out)
    frm = getattr(args, "from")
    r = analyse(os.path.join(args.out, "coast.trace"), frm, args.rsf_car, args.to, args.max_off, args.drag)
    print(f"{args.rsf_car} on {args.stage or args.rsf_stage} {' '.join(args.extra)}, {args.physics}, declutched coast "
          f"from {frm * 3.6:.0f} km/h")
    if not r["ok"]:
        print(r["reason"])
        return
    print(f"crr {r['crr']:.4f} ±{r['spread']:.4f} at b {r['drag'] * 1e3:.4f}e-3 /m; free fit crr {r['crr_free']:.4f} "
          f"b {r['b_free'] * 1e3:.4f}e-3 /m; {r['n']}/{r['of']} pieces straight, {r['outliers']} off the line; "
          f"v {r['v'][0]:.1f}-{r['v'][1]:.1f} m/s, grade {r['grade'] * 100:+.2f} %")


if __name__ == "__main__":
    main()
