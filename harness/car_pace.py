#!/usr/bin/env python3
"""Analyse straight-line pace traces into the sweep CSV.

Each trace is one `--autopilot ... --flat-out --trace-speed` drive: a flat-out
standing start, one line per physics step
(`step time speed thr brk gear engine x y z off str`). For every car this
prints the launch, the valid flat-out window and the pace metrics; with
`--csv` it writes one row per car.

The window starts at the first sustained motion and ends at the first of: the
brake pedal moving (sustained), the car leaving the driveline (sustained), or
sustained hard steering past the launch correction (a corner the car threads
on the road). Everything after the window (slides, crashes, relaunches) is
ignored, so the metrics only cover full-throttle straight driving.

With <car>.circ.trace present (a `--circle X,Y,R` ramp run) it adds the
steady-state circle columns: the first latched hold's fastest held speed and
v^2/R grip. With <car>.brk100/.brk150.trace present (flat-out `--brake-from`
runs) it adds stopping distance and mean deceleration from 100/150 km/h;
crash-stops (a wall, not braking) and unreached triggers stay empty.
"""

import argparse
import csv
import math
import os
import sys

STEP_WINDOW = 720  # NGP steps per second (1/720 s)
SUSTAIN_SHORT = 36  # 0.05 s
SUSTAIN_STEER = 720  # 1 s
SLOWDROP_WINDOW = 360  # 0.5 s
SLOWDROP = 3.0  # m/s lost over the window at full throttle (a slide or crash)
LAUNCH_SPEED = 0.5  # m/s
BRAKE_ON = 0.02
OFF_ROAD = 4.0  # m from the driveline
STEER_HARD = 0.45  # sustained past the first 2 s (the launch correction is over by then)

# Steady-state circle: map 85 school tarmac, CCW, commanded radius 40 m.
CIRC_CX = -260.0
CIRC_CY = 120.0
CIRC_R = 40.0
# Steady-state circle on dry gravel: RSF 345's open start area, CCW, same radius.
GCIRC_CX = 1051.0
GCIRC_CY = -166.0
GCIRC_R = 40.0
CIRC_LATCH = 1.5  # m from the commanded radius: latched on
CIRC_BREAK = 3.0  # m from the commanded radius: the slide (sustained)
CIRC_LATCH_NEED = 2 * STEP_WINDOW  # 2 s on the circle before it counts
CIRC_BREAK_NEED = STEP_WINDOW // 2  # 0.5 s off it ends the hold
CIRC_CRAWL = 25.0 / 3.6  # m/s: a hold peaking below this never got going (bogged or power-capped)
CIRC_DECAY = 1.0  # m/s per s: a hold decaying faster than this (full throttle) is bogging down, not holding
BRAKE100 = 100.0 / 3.6
BRAKE150 = 150.0 / 3.6
BRK_ONSET = 0.05  # sustained brake-pedal travel: braking has started
STOP_SPEED = 0.35  # m/s, sustained: the car has stopped (the trace prints 3 decimals)
CRASH_DROP = 25.0  # km/h lost in 0.25 s: a wall, not braking (peaks ~15)
CRASH_OFF = 4.0  # m off the driveline while braking: left the road (gravel stops wander to 3 m and stay valid)


def load_trace(path):
    """A trace file as a list of dicts."""
    rows = []
    with open(path) as handle:
        for line in handle:
            if line.startswith("#") or not line.strip():
                continue
            fields = line.split()
            rows.append(
                {
                    "step": int(fields[0]),
                    "t": float(fields[1]),
                    "v": float(fields[2]),
                    "thr": float(fields[3]),
                    "brk": float(fields[4]),
                    "gear": int(fields[5]),
                    "eng": float(fields[6]),
                    "x": float(fields[7]),
                    "y": float(fields[8]),
                    "off": float(fields[10]),
                    "str": float(fields[11]),
                }
            )
    return rows


def sustained(flags, need):
    """First index where `flags` holds for `need` in a row, else None."""
    run = 0
    for i, flag in enumerate(flags):
        run = run + 1 if flag else 0
        if run >= need:
            return i - need + 1
    return None


def analyse(rows):
    """The launch frame, the window end and the pace metrics, or a failure."""
    count = len(rows)
    speed = [row["v"] for row in rows]
    launch = None
    for i in range(count - STEP_WINDOW):
        if speed[i] > LAUNCH_SPEED and min(speed[i : i + STEP_WINDOW]) > 0.4:
            launch = i
            break
    if launch is None:
        return {"ok": False, "reason": "no launch (never moved)"}
    origin = rows[launch]["t"]
    rest = range(launch, count)

    brake = sustained([rows[i]["brk"] > BRAKE_ON for i in rest], SUSTAIN_SHORT)
    off = sustained([rows[i]["off"] > OFF_ROAD for i in rest], SUSTAIN_SHORT)
    steer = sustained(
        [
            rows[i]["t"] - origin > 2.0 and abs(rows[i]["str"]) > STEER_HARD
            for i in rest
        ],
        SUSTAIN_STEER,
    )
    # A speed collapse at full throttle: the car slid or crashed (gear shifts
    # cost well under a metre per second over the window).
    slow = None
    peak = 0.0
    for k, i in enumerate(rest):
        peak = max(peak, speed[i])
        if k >= SLOWDROP_WINDOW:
            behind = max(speed[i - SLOWDROP_WINDOW : i])
            if behind - speed[i] > SLOWDROP and rows[i]["thr"] > 0.5:
                slow = k
                break
    ends = {
        "brake": launch + brake if brake is not None else count,
        "off-road": launch + off if off is not None else count,
        "steering": launch + steer if steer is not None else count,
        "spin": launch + slow if slow is not None else count,
    }
    end = min(ends.values())
    why = next((name for name, i in ends.items() if i == end), "end")
    if end >= count:
        why = "end"
        end = count

    def cross(kmh):
        want = kmh / 3.6
        for i in range(launch, end):
            if speed[i] >= want:
                return rows[i]["t"] - origin
        return None

    def at(seconds):
        # The second after the sample must be clean too, so a sample on the
        # edge of a slide or crash reads empty rather than mid-crash.
        i = launch + seconds * STEP_WINDOW
        return speed[i] * 3.6 if i + STEP_WINDOW <= end else None

    top = max(speed[launch:end]) if end > launch else 0.0
    itop = speed.index(top, launch, end) if end > launch else launch
    dist = math.hypot(rows[end - 1]["x"] - rows[launch]["x"], rows[end - 1]["y"] - rows[launch]["y"])

    # A launch bog: the deepest speed drawdown from its running peak over the
    # first 5 s at full throttle (a stall or clutch bog costs seconds).
    peak = 0.0
    drawdown = 0.0
    for i in range(launch, min(launch + 5 * STEP_WINDOW, end)):
        peak = max(peak, speed[i])
        if peak > 1.0:
            drawdown = max(drawdown, (peak - speed[i]) / peak)

    return {
        "ok": True,
        "launch": launch,
        "step0": rows[launch]["step"],
        "tend": rows[end - 1]["t"] - origin,
        "why": why,
        "t60": cross(60),
        "t100": cross(100),
        "t140": cross(140),
        "t160": cross(160),
        "v10": at(10),
        "v20": at(20),
        "v30": at(30),
        "vtop": top * 3.6,
        "ttop": rows[itop]["t"] - origin,
        "dist": dist,
        "bog": drawdown,
    }


def find_launch(speed):
    """First index of sustained motion, else None."""
    for i in range(len(speed) - STEP_WINDOW):
        if speed[i] > LAUNCH_SPEED and min(speed[i : i + STEP_WINDOW]) > 0.4:
            return i
    return None


def mean_speed(speed, i, need=STEP_WINDOW):
    """Mean speed over `need` steps from i, else None at the trace end."""
    if i + need > len(speed):
        return None
    return sum(speed[i : i + need]) / need


def analyse_circle(rows, cx=CIRC_CX, cy=CIRC_CY, radius=CIRC_R):
    """Steady-state circle: the first latched hold to its first slide.

    The car latches on (stays within CIRC_LATCH of the commanded radius for
    2 s) while the target speed ramps up, then slides (stays past CIRC_BREAK
    for 0.5 s). v_max is the fastest 1 s-mean speed inside that first hold
    only, so a post-spin rejoin cannot inflate it; without a slide the hold
    runs to the trace end (flagged). Lateral grip is v^2/R on the commanded
    radius."""
    count = len(rows)
    speed = [row["v"] for row in rows]
    launch = find_launch(speed)
    if launch is None:
        return {"ok": False, "reason": "no launch (never moved)"}
    offc = [math.hypot(row["x"] - cx, row["y"] - cy) - radius for row in rows]
    # The latch needs the car to travel the circle, not spin in place (a spin holds its position while
    # the speed collapses): the unwrapped angle about the centre must advance over the window.
    ang = [math.atan2(row["y"] - cy, row["x"] - cx) for row in rows]
    latch = None
    for i in range(launch, count - CIRC_LATCH_NEED):
        if all(abs(offc[j]) < CIRC_LATCH for j in range(i, i + CIRC_LATCH_NEED)):
            turn = sum((ang[j + 1] - ang[j] + math.pi) % (2 * math.pi) - math.pi
                       for j in range(i, i + CIRC_LATCH_NEED - 1))
            if abs(turn) > 0.4:
                latch = i
                break
    if latch is None:
        return {"ok": False, "reason": "never latched onto the circle"}
    if rows[latch]["t"] - rows[launch]["t"] > 15.0:
        # A clean approach latches in a few seconds; a later latch follows slides or spins (a hot rejoin).
        return {"ok": False, "reason": "latched too late (after slides)"}
    if sustained([v < 3.0 for v in speed[launch:latch]], STEP_WINDOW) is not None:
        # Stopped before latching: beached and relaunched, so the hold is a recovery acceleration.
        return {"ok": False, "reason": "stopped before latching (beach and relaunch)"}
    rest = range(latch, count)
    brk = sustained([offc[i] > CIRC_BREAK or offc[i] < -CIRC_BREAK for i in rest],
                    CIRC_BREAK_NEED)
    slid = brk + latch if brk is not None else count
    best = 0.0
    ibest = latch
    i = latch
    while i + STEP_WINDOW <= min(slid, count):
        mean = mean_speed(speed, i)
        if mean > best:
            best, ibest = mean, i
        i += STEP_WINDOW // 4
    if best < CIRC_CRAWL:
        return {"ok": False, "reason": "never got going on the circle (bogged or power-capped)"}
    # A hold decaying under full throttle is bogging down, not holding: fit the speed slope over the
    # hold (excluding the last second, where a genuine slide also scrubs speed).
    hold_end = max(latch + STEP_WINDOW, min(slid, count) - STEP_WINDOW)
    n_fit = max(2, (hold_end - latch) // (STEP_WINDOW // 2))
    xs = [k for k in range(n_fit)]
    ys = [speed[latch + k * (STEP_WINDOW // 2)] for k in range(n_fit)]
    slope = 0.0
    if hold_end - latch >= 2 * STEP_WINDOW:
        mx = sum(xs) / n_fit
        denom = sum((x - mx) ** 2 for x in xs)
        if denom > 0:
            slope = sum((x - mx) * (y - sum(ys) / n_fit) for x, y in zip(xs, ys)) / denom
            slope *= STEP_WINDOW / 2  # per sample (0.5 s) to per second
    mean_thr = sum(rows[k]["thr"] for k in range(latch, hold_end)) / max(1, hold_end - latch)
    if slope < -CIRC_DECAY and mean_thr > 0.8:
        return {"ok": False, "reason": "bogged down on the circle (speed decayed under full throttle)"}
    mean_off = sum(abs(offc[i]) for i in range(latch, min(slid, count)))
    mean_off /= max(1, min(slid, count) - latch)
    return {
        "ok": True,
        "launch": launch,
        "latch": (rows[latch]["t"] - rows[launch]["t"]),
        "vmax": best * 3.6,
        "latg": (best * best / radius) / 9.81,
        "slid": brk is not None,
        "tend": (rows[min(slid, count) - 1]["t"] - rows[launch]["t"]),
        "mean_off": mean_off,
    }


def analyse_brake(rows, frm):
    """Flat-out to `frm` m/s, then full braking to a stop.

    Valid only for a clean stop on the road: a 0.25 s speed drop past
    CRASH_DROP or wandering past CRASH_OFF off the driveline means the car
    hit something (a wall or tree), so the distance is a crash-stop, not
    braking, and the columns stay empty."""
    count = len(rows)
    speed = [row["v"] for row in rows]
    launch = find_launch(speed)
    if launch is None:
        return {"ok": False, "reason": "no launch (never moved)"}
    trig = next((i for i in range(launch, count) if speed[i] >= frm), None)
    if trig is None:
        return {"ok": False, "reason": f"never reached {frm * 3.6:.0f} km/h",
                "vmax": max(speed[launch:]) * 3.6}
    onset = next((i for i in range(trig, count) if rows[i]["brk"] > BRK_ONSET), None)
    if onset is None:
        return {"ok": False, "reason": "brake never engaged"}
    # The run ends at the stop, so only a few slow frames exist; require 6
    # in a row (a lone dip mid-braking is not a stop).
    stop = next((i for i in range(onset, count)
                 if speed[i] < STOP_SPEED and
                 all(v < STOP_SPEED for v in speed[i:i + 6])), None)
    if stop is None:
        return {"ok": False, "reason": "never stopped"}
    worst_drop = 0.0
    for i in range(onset, stop - STEP_WINDOW // 4):
        drop = (speed[i] - speed[i + STEP_WINDOW // 4]) * 3.6
        worst_drop = max(worst_drop, drop)
    max_off = max(abs(rows[i]["off"]) for i in range(onset, stop + 1))
    dist = math.hypot(rows[stop]["x"] - rows[onset]["x"],
                      rows[stop]["y"] - rows[onset]["y"])
    if worst_drop > CRASH_DROP or max_off > CRASH_OFF:
        return {"ok": False, "reason": "hit something while braking",
                "vmax": speed[trig] * 3.6, "drop": worst_drop, "off": max_off}
    v0 = speed[onset]
    return {
        "ok": True,
        "launch": launch,
        "trig_v": speed[trig] * 3.6,
        "dist": dist,
        "time": rows[stop]["t"] - rows[onset]["t"],
        "decel": (v0 * v0) / (2 * dist) if dist > 0 else 0.0,
    }


def end_reason(log_path):
    """The drive's "ended by ..." line, if the log has one."""
    try:
        with open(log_path) as handle:
            for line in handle:
                if "ended by " in line:
                    return line.split("ended by ", 1)[1].strip()
    except OSError:
        pass
    return None


def fmt_time(value):
    return "" if value is None else f"{value:.2f}"


def fmt_speed(value):
    return "" if value is None else f"{value:.1f}"


def note(result, ended):
    """The CSV notes: where the flat-out window ends, and launch remarks."""
    if not result["ok"]:
        text = result["reason"]
        return f"{text}; run {ended}" if ended else text
    parts = []
    if result["why"] == "end":
        parts.append(f"flat-out to the run's end ({result['tend']:.1f}s)")
    else:
        parts.append(f"flat-out to {result['tend']:.1f}s ({result['why']})")
    if result["tend"] - result["ttop"] < 1.0:
        parts.append("still accelerating")
    if result["bog"] > 0.25:
        parts.append(f"launch bog (drawdown {result['bog']:.2f})")
    if ended and result["why"] == "end":
        parts.append(f"run {ended}")
    elif ended:
        parts.append(f"then {ended}")
    return "; ".join(parts)


def circle_note(result):
    """Compact circle remark for the CSV notes."""
    if result is None:
        return "circ r40 not run"
    if not result["ok"]:
        return f"circ r40: {result['reason']}"
    held = f"slide@{result['vmax']:.1f} ({result['latg']:.2f}g)" if result["slid"] \
        else f"held@{result['vmax']:.1f} to run end (no slide)"
    wide = f", off {result['mean_off']:.1f}m" if result["mean_off"] > 1.0 else ""
    return f"circ r40 {held}{wide}"


def brake_note(result, kmh, prefix="brk"):
    """Compact brake remark for the CSV notes."""
    if result is None:
        return f"{prefix}{kmh} not run"
    if not result["ok"]:
        return f"{prefix}{kmh}: {result['reason']}"
    return f"{prefix}{kmh} {result['dist']:.1f}m {result['decel'] / 9.81:.2f}g"


def gravel_notes(gcirc, gb100):
    """Compact gravel remarks for the CSV notes."""
    parts = []
    if gcirc is None:
        parts.append("gcirc r40 not run")
    elif not gcirc["ok"]:
        parts.append(f"gcirc r40: {gcirc['reason']}")
    else:
        held = f"slide@{gcirc['vmax']:.1f} ({gcirc['latg']:.2f}g)" if gcirc["slid"] \
            else f"held@{gcirc['vmax']:.1f} to run end (no slide)"
        parts.append(f"gcirc r40 {held}")
    parts.append(brake_note(gb100, 100, "gbrk"))
    return "; ".join(parts)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True,
                        help="TSV: car, kind, key, name, class, extra")
    parser.add_argument("--traces", required=True, help="directory of <car>.trace (+<car>.log)")
    parser.add_argument("--csv", required=True, help="output CSV path")
    parser.add_argument("--surface", default="tarmac")
    parser.add_argument("--stage", default="dry tarmac: 504-O pace, 568-M brakes, map85 circle")
    parser.add_argument("--qualify", action="store_true",
                        help="print cars whose pace vtop reaches 150 km/h (brake-150 candidates)")
    args = parser.parse_args()

    with open(args.manifest) as handle:
        cars = [line.rstrip("\n").split("\t") for line in handle if line.strip()]

    def maybe_trace(car, suffix):
        path = os.path.join(args.traces, f"{car}{suffix}.trace")
        return load_trace(path) if os.path.exists(path) else None

    analyses = []
    circles = []
    brakes100 = []
    brakes150 = []
    gcircles = []
    gbrakes100 = []
    for car, _kind, _key, _name, _group, _extra in cars:
        rows = load_trace(os.path.join(args.traces, f"{car}.trace"))
        analyses.append(analyse(rows))
        circ = maybe_trace(car, ".circ")
        circles.append(analyse_circle(circ) if circ else None)
        b100 = maybe_trace(car, ".brk100")
        brakes100.append(analyse_brake(b100, BRAKE100) if b100 else None)
        b150 = maybe_trace(car, ".brk150")
        brakes150.append(analyse_brake(b150, BRAKE150) if b150 else None)
        gcirc = maybe_trace(car, ".gcirc")
        gcircles.append(analyse_circle(gcirc, GCIRC_CX, GCIRC_CY, GCIRC_R) if gcirc else None)
        gb100 = maybe_trace(car, ".gbrk100")
        gbrakes100.append(analyse_brake(gb100, BRAKE100) if gb100 else None)
    if args.qualify:
        for (car, _k, _kk, _n, _g, _e), result in zip(cars, analyses):
            if result.get("ok") and result["vtop"] >= 150.0:
                print(car)
        return
    launched = sorted(a["launch"] for a in analyses if a.get("ok"))
    median_launch = launched[len(launched) // 2] if launched else 0

    with open(args.csv, "w", newline="") as handle:
        out = csv.writer(handle)
        out.writerow(["car", "name", "class", "surface", "stage", "t_0_60", "t_0_100", "t_0_140",
                      "t_0_160", "v10", "v20", "v30", "vtop", "dist", "v_max_r40", "lat_g_r40",
                      "brake_100_m", "brake_100_decel", "brake_150_m", "brake_150_decel",
                      "v_max_r40_gravel", "lat_g_r40_gravel",
                      "brake_100_m_gravel", "brake_100_decel_gravel", "notes"])
        rows_out = zip(cars, analyses, circles, brakes100, brakes150, gcircles, gbrakes100)
        for entry, result, circ, b100, b150, gcirc, gb100 in rows_out:
            car, _kind, _key, name, group, extra = entry
            ended = end_reason(os.path.join(args.traces, f"{car}.log"))
            text = note(result, ended)
            if result.get("ok") and result["launch"] - median_launch > STEP_WINDOW:
                late = (result["launch"] - median_launch) / STEP_WINDOW
                text += f"; late launch (+{late:.1f}s)"
            text += f"; {circle_note(circ)}; {brake_note(b100, 100)}; {brake_note(b150, 150)}"
            text += f"; {gravel_notes(gcirc, gb100)}"
            if extra:
                text = f"{extra}; {text}" if text else extra
            if not result.get("ok"):
                out.writerow([car, name, group, args.surface, args.stage,
                              "", "", "", "", "", "", "", "", "", "", "",
                              "", "", "", "", "", "", "", "", text])
                continue
            out.writerow([car, name, group, args.surface, args.stage,
                          fmt_time(result["t60"]), fmt_time(result["t100"]),
                          fmt_time(result["t140"]), fmt_time(result["t160"]),
                          fmt_speed(result["v10"]), fmt_speed(result["v20"]),
                          fmt_speed(result["v30"]), fmt_speed(result["vtop"]),
                          f"{result['dist']:.0f}",
                          fmt_speed(circ["vmax"]) if circ and circ.get("ok") else "",
                          f"{circ['latg']:.3f}" if circ and circ.get("ok") else "",
                          f"{b100['dist']:.1f}" if b100 and b100.get("ok") else "",
                          f"{b100['decel'] / 9.81:.2f}" if b100 and b100.get("ok") else "",
                          f"{b150['dist']:.1f}" if b150 and b150.get("ok") else "",
                          f"{b150['decel'] / 9.81:.2f}" if b150 and b150.get("ok") else "",
                          fmt_speed(gcirc["vmax"]) if gcirc and gcirc.get("ok") else "",
                          f"{gcirc['latg']:.3f}" if gcirc and gcirc.get("ok") else "",
                          f"{gb100['dist']:.1f}" if gb100 and gb100.get("ok") else "",
                          f"{gb100['decel'] / 9.81:.2f}" if gb100 and gb100.get("ok") else "",
                          text])


if __name__ == "__main__":
    main()
