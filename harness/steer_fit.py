"""steer_fit.py: fit the speed-shaped steering response k(speed) to the lock a driver really uses.

The response maps stick travel x in [0,1] to steering y = x + (x^2 - x) * k (k = 0 linear, k = 1 is y = x^2). Each
part of the travel serves an equal share of the steering used at that speed when y(p) = q(p), the quantile p of
|steering|. Per speed bin the k minimizes sum_p (p + (p^2 - p) k - q(p) / qmax)^2 for p = 0.05..0.95, k in [0,1];
then k(speed) = clamp((speed - v0) * slope, 0, 1) is fitted over the bins, weighted by sample count.

  python3 -I -B harness/steer_fit.py SURFACE=CSV[,CSV...] ... [--bin 2] [--min-samples 500]

The CSVs come from `cargo run --release -p rbr-recording --example steer_speed -- physics.bin` (step,steering,speed,
faded_speed). Steps under 1 m/s are dropped (before the start, stopped). Prints the per-bin tables per surface and
pooled for both speeds and both normalisations (qmax = the bin's 0.99 quantile, or 1.0), then the k(speed) fits.
"""
import argparse

import numpy as np

PS = np.arange(0.05, 0.951, 0.05)
MIN_SPEED = 1.0


def load(spec):
    surface, _, files = spec.partition("=")
    rows = [np.loadtxt(f, delimiter=",", skiprows=1, ndmin=2) for f in files.split(",")]
    data = np.vstack([r for r in rows if r.size])
    data = data[data[:, 2] >= MIN_SPEED]
    return surface, data  # columns: step, steering, speed, faded


def fit_k(q):
    """Least-squares k in [0,1] of y(p) = p - p(1-p)k against q (normalised), closed form."""
    w = PS * (1 - PS)
    return float(np.clip(np.sum(w * (PS - q)) / np.sum(w * w), 0.0, 1.0))


def fit_power(q):
    """Exponent a of y = p^a (log-log least squares through the origin): how much steeper than x^2 (a = 2) the
    quantiles are; k is capped at 1, so a above 2 is what k cannot reach."""
    ok = q > 1e-6
    return float(np.sum(np.log(PS[ok]) * np.log(q[ok])) / np.sum(np.log(PS[ok]) ** 2))


def bins(data, column, width, min_samples, qmax_mode):
    """[(lo, n, median, p90, k)] over speed bins of the chosen speed column (2 forward, 3 faded)."""
    speed, steer = data[:, column], np.abs(data[:, 1])
    out = []
    for lo in np.arange(0.0, np.ceil(speed.max() / width) * width, width):
        s = steer[(speed >= lo) & (speed < lo + width)]
        if len(s) < min_samples:
            continue
        q = np.quantile(s, PS)
        qmax = np.quantile(s, 0.99) if qmax_mode == "q99" else 1.0
        out.append((lo, len(s), float(np.median(s)), float(np.quantile(s, 0.9)), fit_k(q / max(qmax, 1e-9)), float(qmax), fit_power(q / max(qmax, 1e-9))))
    return out


def fit_curve(table):
    """(v0, slope, error) of k = clamp((v - v0) * slope, 0, 1) over the bin centres, weighted by count."""
    if not table:
        return None
    v = np.array([b[0] for b in table]) + 0.0
    n, k = np.array([b[1] for b in table], float), np.array([b[4] for b in table])
    best = None
    for v0 in np.arange(0.0, 40.0, 0.5):
        for slope in np.arange(0.005, 2.0, 0.005):
            err = float(np.sum(n * (np.clip((v - v0) * slope, 0, 1) - k) ** 2) / n.sum())
            if best is None or err < best[2]:
                best = (float(v0), float(slope), err)
    return best


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("sets", nargs="+", help="SURFACE=file.csv[,file.csv...]")
    ap.add_argument("--bin", type=float, default=2.0, help="speed bin width, m/s")
    ap.add_argument("--min-samples", type=int, default=500)
    a = ap.parse_args()
    sets = dict(load(s) for s in a.sets)
    sets["pooled"] = np.vstack(list(sets.values()))
    for mode in ("q99", "1.0"):
        for name, column in (("forward", 2), ("faded", 3)):
            print(f"\n=== speed: {name}, qmax: {mode} ===")
            for surface, data in sets.items():
                table = bins(data, column, a.bin, a.min_samples, mode)
                print(f"-- {surface}: {len(data)} steps >= {MIN_SPEED} m/s")
                print("  bin m/s      n   median    p90    qmax      k  power")
                for lo, n, med, p90, k, qmax, pw in table:
                    print(f"  {lo:4.0f}-{lo + a.bin:<4.0f} {n:7d}  {med:6.3f}  {p90:6.3f}  {qmax:6.3f}  {k:5.2f}  {pw:5.2f}")
                fit = fit_curve(table)
                if fit:
                    v0, slope, err = fit
                    full = v0 + 1 / slope
                    print(f"  fit: k = clamp((v - {v0:.1f}) * {slope:.3f}, 0, 1)  (k=1 from {full:.1f} m/s, rms {err ** .5:.3f})")


main()
