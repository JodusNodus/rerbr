"""engine_scorecard.py: score the realistic engine model (rbr-engine) against the published targets in
data/engine-targets.csv.

  python3 -I -B harness/engine_scorecard.py [--only REGEX] [--json OUT]
  python3 -I -B harness/engine_scorecard.py --compare A.json B.json

Every row is measured by the rbr-engine `engine_scorecard` example (built and run here) and printed with its band,
PASS or MISS and how far outside the band it lies (bands are inclusive; an empty bound is open). A row with both
bounds empty is context: measured and printed (CTX), not scored. The `how` keys:

  SAMPLE:KEY              a measurement of the model calibrated on SAMPLE: the synthetic samples (na, turbo, old,
                          cable_turbo; `rbr_engine::samples`) or an installed RSF car's folder (needs RBR_RSF, else
                          n/a). Warm, reference air, steady at a fixed speed: closed_bmep_N and closed_map_N (a closed
                          pedal at N rpm come down from 300 rpm under the limit, so the overrun cut has armed),
                          closed_limit_ratio (the closed pedal with the fuel cut at RPMLimit over NGP's
                          BrakeCoefficient·MaxTorque), cold_friction_2000 (FMEP at 20 °C oil over 90 °C),
                          zero_map_2500 (the manifold pressure of zero net torque), bmep_P_N (BMEP with the manifold
                          at P kPa), share_50_2500 (torque at 50 kPa over 100 kPa, %), eta_ig_N (K·AFR·λ/LHV),
                          idle_map, idle_exhaust (at IdleRPM, closed pedal), fired_map_min (the lowest manifold
                          pressure at a closed pedal with the fuel on, idle + 50..700 rpm), cut_exhaust_3000 (the
                          closed_bmep_3000 run's exhaust), exhaust_015_1500 (pedal 0.15), exhaust_rated (WOT at the
                          grid speed of the most power), spool_first (5 bar BMEP to 90 % of the steady WOT torque at the
                          first 500 rpm step with 1.5 bar of steady WOT boost)
  model:KEY               the model's constants: bearing_loss_80k (W), inertia (kg·m²), lambda
  class:GROUP:YEAR:restrictor_mm  the class's restrictor for a turbo of that group and year, mm (0: none)

--json writes the values per id, the raw measurements, the commit and the bands; --compare prints the changes between
two such files, each scored on its own bands (a changed band is flagged). RBR_RSF comes from dev.env."""
import argparse
import csv
import json
import math
import os
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import dev_env  # noqa: E402,F401  (dev.env: RBR_RSF)

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TARGETS = os.path.join(REPO, "data", "engine-targets.csv")
SAMPLES = {"na", "turbo", "old", "cable_turbo", "model", "class"}


def load_targets():
    with open(TARGETS, newline="") as f:
        return list(csv.DictReader(f))


def bound(text):
    return float(text) if text.strip() else None


def verdict(value, low, high):
    """PASS or MISS and the signed distance outside the band (0 inside); CTX for a context row (no band)."""
    if low is None and high is None:
        return "CTX", 0.0
    if math.isnan(value):
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


def measure(rows):
    """Runs the example over the rows' cars and classes; the measurements by key."""
    cars = sorted({r["how"].split(":")[0] for r in rows} - SAMPLES)
    if cars and not os.environ.get("RBR_RSF"):
        print(f"RBR_RSF not set: {', '.join(cars)} n/a", file=sys.stderr)
        cars = []
    classes = sorted({r["how"].rsplit(":", 1)[0] for r in rows if r["how"].startswith("class:")})
    cmd = ["cargo", "run", "--release", "-q", "-p", "rbr-engine", "--example", "engine_scorecard", "--", *cars,
           *classes]
    out = subprocess.run(cmd, cwd=REPO, check=True, capture_output=True, text=True).stdout
    raw = {}
    for line in out.splitlines():
        key, _, value = line.rpartition(" ")
        raw[key] = float(value)
    return raw


def score(args):
    rows = load_targets()
    if args.only:
        rows = [r for r in rows if re.search(args.only, r["id"]) or re.search(args.only, r["how"])]
    if not rows:
        sys.exit("no rows selected")
    raw = measure(rows)
    values = {}
    print(f"{'id':40} {'value':>10} {'band':>15} {'':4} {'out':>9}")
    for row in rows:
        low, high = bound(row["low"]), bound(row["high"])
        value = raw.get(row["how"])
        values[row["id"]] = value
        if value is None:
            print(f"{row['id']:40} {'n/a':>10} {band_text(low, high):>15}")
            continue
        status, out = verdict(value, low, high)
        print(f"{row['id']:40} {value:10.4g} {band_text(low, high):>15} {status:4} "
              f"{'' if status != 'MISS' else f'{out:+9.3g}':>9}")
    scored = [r for r in rows if bound(r["low"]) is not None or bound(r["high"]) is not None]
    passed = sum(1 for r in scored if values[r["id"]] is not None and
                 verdict(values[r["id"]], bound(r["low"]), bound(r["high"]))[0] == "PASS")
    measured = sum(1 for r in scored if values[r["id"]] is not None)
    print(f"{passed}/{len(scored)} PASS" + (f" ({len(scored) - measured} n/a)" if measured < len(scored) else ""))
    by_system = {}
    for r in scored:
        if values[r["id"]] is None:
            continue
        ok = verdict(values[r["id"]], bound(r["low"]), bound(r["high"]))[0] == "PASS"
        n = by_system.setdefault(r["system"], [0, 0])
        n[0] += ok
        n[1] += 1
    print("  " + ", ".join(f"{s} {p}/{n}" for s, (p, n) in by_system.items()))
    if args.json:
        out = {"commit": commit(), "settings": {"only": args.only, "targets": os.path.relpath(TARGETS, REPO)},
               "bands": {r["id"]: [bound(r["low"]), bound(r["high"])] for r in rows}, "values": values, "raw": raw}
        os.makedirs(os.path.dirname(os.path.abspath(args.json)), exist_ok=True)
        with open(args.json, "w") as f:
            json.dump(out, f, indent=1, sort_keys=True)


def compare(a_path, b_path):
    """Each file's values against its own stored bands; a row whose band changed is printed with both bands and
    `band` in its change."""
    with open(a_path) as f:
        a = json.load(f)
    with open(b_path) as f:
        b = json.load(f)
    print(f"A {a_path} ({a['commit']})\nB {b_path} ({b['commit']})")
    print(f"{'id':40} {'A':>10} {'B':>10} {'B-A':>10} {'band':>15}  change")
    only = sorted(set(a["values"]) ^ set(b["values"]))
    if only:
        print(f"measured in one file only (skipped): {', '.join(only)}")
    counts = {"A": 0, "B": 0, "scored": 0}
    for rid in sorted(set(a["values"]) & set(b["values"])):
        band_a, band_b = tuple(a["bands"][rid]), tuple(b["bands"][rid])
        va, vb = a["values"][rid], b["values"][rid]
        sa = verdict(va, *band_a)[0] if va is not None else "n/a"
        sb = verdict(vb, *band_b)[0] if vb is not None else "n/a"
        if band_b != (None, None):
            counts["scored"] += 1
            counts["A"] += sa == "PASS"
            counts["B"] += sb == "PASS"
        moved = band_a != band_b
        close = va is not None and vb is not None and (abs(vb - va) <= 1e-6 * max(1.0, abs(va)) or
                                                       math.isnan(va) and math.isnan(vb))
        if close and sa == sb and not moved:
            continue
        fa = f"{va:10.4g}" if va is not None else f"{'n/a':>10}"
        fb = f"{vb:10.4g}" if vb is not None else f"{'n/a':>10}"
        fd = f"{vb - va:+10.3g}" if va is not None and vb is not None else f"{'':>10}"
        change = f"{sa} -> {sb}" if sa != sb else sb
        band = band_text(*band_b)
        if moved:
            band, change = f"{band_text(*band_a)} -> {band}", f"{change} (band)"
        print(f"{rid:40} {fa} {fb} {fd} {band:>15}  {change}")
    print(f"PASS {counts['A']} -> {counts['B']} of {counts['scored']} rows in both")


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--only", help="regex over the ids and hows")
    parser.add_argument("--json")
    parser.add_argument("--compare", nargs=2, metavar=("A", "B"))
    args = parser.parse_args()
    if args.compare:
        compare(*args.compare)
    else:
        score(args)


if __name__ == "__main__":
    main()
