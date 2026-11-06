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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True,
                        help="TSV: car, kind, key, name, class, extra")
    parser.add_argument("--traces", required=True, help="directory of <car>.trace (+<car>.log)")
    parser.add_argument("--csv", required=True, help="output CSV path")
    parser.add_argument("--surface", default="tarmac")
    parser.add_argument("--stage", default="RSF 504 Witoszow-Zloty-Las (O)")
    args = parser.parse_args()

    with open(args.manifest) as handle:
        cars = [line.rstrip("\n").split("\t") for line in handle if line.strip()]

    analyses = []
    for car, _kind, _key, _name, _group, _extra in cars:
        rows = load_trace(os.path.join(args.traces, f"{car}.trace"))
        analyses.append(analyse(rows))
    launched = sorted(a["launch"] for a in analyses if a.get("ok"))
    median_launch = launched[len(launched) // 2] if launched else 0

    with open(args.csv, "w", newline="") as handle:
        out = csv.writer(handle)
        out.writerow(["car", "name", "class", "surface", "stage", "t_0_60", "t_0_100", "t_0_140",
                      "t_0_160", "v10", "v20", "v30", "vtop", "dist", "notes"])
        for (car, _kind, _key, name, group, extra), result in zip(cars, analyses):
            ended = end_reason(os.path.join(args.traces, f"{car}.log"))
            text = note(result, ended)
            if result.get("ok") and result["launch"] - median_launch > STEP_WINDOW:
                late = (result["launch"] - median_launch) / STEP_WINDOW
                text += f"; late launch (+{late:.1f}s)"
            if extra:
                text = f"{extra}; {text}" if text else extra
            if not result.get("ok"):
                out.writerow([car, name, group, args.surface, args.stage,
                              "", "", "", "", "", "", "", "", "", text])
                continue
            out.writerow([car, name, group, args.surface, args.stage,
                          fmt_time(result["t60"]), fmt_time(result["t100"]),
                          fmt_time(result["t140"]), fmt_time(result["t160"]),
                          fmt_speed(result["v10"]), fmt_speed(result["v20"]),
                          fmt_speed(result["v30"]), fmt_speed(result["vtop"]),
                          f"{result['dist']:.0f}", text])


if __name__ == "__main__":
    main()
