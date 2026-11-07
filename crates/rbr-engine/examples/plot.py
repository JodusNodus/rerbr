"""Plots of examples/curves.rs's CSVs.

python3 -I plot.py DIR [car:litres ...]: a panel sheet per car, DIR/<car>.png.
python3 -I plot.py --figures DIR OUT: the figures of docs/engine-model.md from the sample cars' CSVs.
"""
import csv, sys, os, collections
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

# rF2 SkipBarber 2.0 L coast torque (rF2 engine.ini doc), N·m at rpm.
RF2 = [(2000, -20.8), (4000, -29.9), (6000, -40.7), (8000, -52.6)]
def per_car(d, litres):
    for f in sorted(os.listdir(d)):
        if not f.endswith("_steady.csv"):
            continue
        car = f[: -len("_steady.csv")]
        st = list(csv.DictReader(open(os.path.join(d, f))))
        tr = list(csv.DictReader(open(os.path.join(d, car + "_transients.csv"))))
        by = collections.defaultdict(list)
        for r in tr:
            by[r["scenario"]].append(r)
        fig, axes = plt.subplots(3, 4, figsize=(24, 14))
        ax = axes[0][0]
        pedals = sorted({float(r["pedal"]) for r in st})
        for p in pedals:
            rows = [r for r in st if float(r["pedal"]) == p]
            x = [float(r["rpm"]) for r in rows]
            l, = ax.plot(x, [float(r["torque"]) for r in rows], label=f"{p:.1f}")
            if p in (0.0, 1.0):
                ax.plot(x, [float(r["ngp"]) for r in rows], "--", color=l.get_color())
        ax.axhline(0, color="k", lw=0.5)
        ax.set_title(f"{car}: steady net torque by pedal (NGP dashed at 0 and 1)")
        ax.set_xlabel("rpm"); ax.set_ylabel("N·m"); ax.legend(fontsize=7, ncol=2)
        ax = axes[0][1]
        rows = [r for r in st if float(r["pedal"]) == 0.0]
        x = [float(r["rpm"]) for r in rows]
        ax.plot(x, [float(r["torque"]) for r in rows], label="model, pedal 0")
        ax.plot(x, [float(r["ngp"]) for r in rows], "--", label="NGP line B")
        if car in litres:
            s = float(litres[car]) / 2.0
            ax.plot([a for a, _ in RF2], [b * s for _, b in RF2], "o-", label=f"rF2 SkipBarber ×{s:.2f}")
        ax.set_title("closed throttle"); ax.set_xlabel("rpm"); ax.set_ylabel("N·m"); ax.legend(fontsize=8)
        panels = [k for k in by]
        for i, name in enumerate(panels[:10]):
            ax = axes[(i + 2) // 4][(i + 2) % 4]
            rows = by[name]
            t = [float(r["time"]) for r in rows]
            ax.plot(t, [float(r["torque"]) for r in rows], label="torque N·m")
            ax.plot(t, [float(r["rpm"]) / 20 for r in rows], label="rpm/20")
            ax.plot(t, [float(r["manifold"]) / 1000 for r in rows], label="MAP kPa")
            if any(float(r["turbo"]) > 0 for r in rows):
                ax.plot(t, [float(r["boost"]) / 1000 for r in rows], label="boost kPa")
                ax.plot(t, [float(r["turbo"]) / 100 for r in rows], label="turbo rad/s /100")
            ax.plot(t, [float(r["pedal"]) * 100 for r in rows], ":", label="pedal %")
            ax.plot(t, [float(r["cut"]) * 100 for r in rows], ":", label="cut %")
            if any(r["anti_lag"] == "1" for r in rows):
                ax.plot(t, [float(r["anti_lag"]) * 50 for r in rows], ":", label="ALS")
            ax.set_title(name); ax.set_xlabel("s"); ax.legend(fontsize=7)
        fig.tight_layout()
        fig.savefig(os.path.join(d, car + ".png"), dpi=70)
        print(os.path.join(d, car + ".png"))


# ---- The figures for docs/engine-model.md: python3 -I plot.py --figures CSV_DIR OUT_DIR (sample cars only). ----

CARS = {"old": "old: Group 4 NA, cable, ITBs, idle stop", "na": "na: modern NA, DBW",
        "turbo": "turbo: Rally2-like, DBW, anti-lag", "cable_turbo": "cable_turbo: Group A turbo, cable, spring gate"}
COLOURS = {"old": "#8c564b", "na": "#1f77b4", "turbo": "#d62728", "cable_turbo": "#ff7f0e", "ngp": "#555555"}
# BeamNG 2023, read from the figure Torque-Throttle.png of https://beamng.com/game/news/blog/throttle-torque-curves/.
BEAMNG = [(10, 49), (20, 65), (30, 76), (40, 84), (50, 91), (60, 95), (70, 98), (80, 99), (90, 100), (100, 100)]
DT = 1.0 / 720.0


def load(d, car):
    st = list(csv.DictReader(open(os.path.join(d, car + "_steady.csv"))))
    tr = collections.defaultdict(list)
    for r in csv.DictReader(open(os.path.join(d, car + "_transients.csv"))):
        tr[r["scenario"]].append({k: (v if k == "scenario" else float(v)) for k, v in r.items()})
    p = {"knots": []}
    for r in csv.reader(open(os.path.join(d, car + "_params.csv"))):
        if r[0] == "knot":
            p["knots"].append((float(r[1]), float(r[2])))
        elif r[0] != "key":
            p[r[0]] = float(r[1])
    st = [{k: float(v) for k, v in r.items()} for r in st]
    return st, tr, p


def table(p, rpm):
    k = p["knots"]
    if rpm <= k[0][0]:
        return k[0][1]
    for (a, ya), (b, yb) in zip(k, k[1:]):
        if rpm <= b:
            return ya + (yb - ya) * (rpm - a) / (b - a) if b > a else yb
    return k[-1][1]


def ngp(p, rpm, pedal):
    """A replica of NGP's engine wrapper with its idle model (not the bit-exact port): below IdleRPM the table's
    torque plus line B, capped to 5 % of MaxTorque below 0.95·IdleRPM (and 1000 rpm); above it (1 − t)·B + t·V with V
    cut to 0 past RPMLimit."""
    mx, lim, idle = p["max_torque"], p["rev_limit"], p["idle"]
    b = -min(rpm, lim) / lim * p["brake"] * mx
    v = table(p, rpm) * mx
    if rpm < idle:
        if rpm < min(1000.0, 0.95 * idle):
            v = min(0.05 * mx, v)
        return v + b
    if rpm > lim:
        v = 0.0
    return (1.0 - pedal) * b + pedal * v


def ngp_neutral(p, rpm0, seconds, pedal, dip=None):
    """NGP's replica in neutral with the car's inertia, stepped as the bench steps ours (torque/1.05/J)."""
    omega_rpm, rows = rpm0, []
    for i in range(int(round(seconds / DT))):
        t = i * DT
        if dip and i == int(round(dip[0] / DT)):
            omega_rpm -= dip[1]
        tq = ngp(p, omega_rpm, pedal(t))
        rows.append((t, omega_rpm, tq))
        omega_rpm = max(0.0, omega_rpm + tq / 1.05 / p["inertia"] * DT * 30.0 / 3.141592653589793)
    return rows


def style(ax, xlabel, ylabel, title=None):
    ax.set_xlabel(xlabel)
    ax.set_ylabel(ylabel)
    if title:
        ax.set_title(title, fontsize=10)
    ax.grid(alpha=0.3)


def figures(d, out):
    os.makedirs(out, exist_ok=True)
    plt.rcParams.update({"font.size": 9, "axes.spines.top": False, "axes.spines.right": False})
    data = {c: load(d, c) for c in CARS if os.path.exists(os.path.join(d, c + "_steady.csv"))}
    written = []

    def save(fig, name):
        fig.tight_layout()
        path = os.path.join(out, name)
        fig.savefig(path, dpi=100)
        plt.close(fig)
        written.append(path)

    # 1. Steady net torque by pedal, ours and NGP's formula.
    for car, (st, _, p) in data.items():
        fig, axes = plt.subplots(1, 2, figsize=(11, 4.2), sharey=True)
        pedals = sorted({r["pedal"] for r in st})
        cmap = plt.get_cmap("viridis")
        # Ours from IdleRPM up: below it the bench holds the idle air wide open, which is the ECU, not the engine.
        titles = ("ours, from IdleRPM up", "NGP's formula (1 − t)·B + t·V")
        for col, (ax, key, title) in enumerate(zip(axes, ("torque", "ngp"), titles)):
            for q in pedals:
                rows = [r for r in st if r["pedal"] == q and (key == "ngp" or r["rpm"] >= p["idle"])]
                ax.plot([r["rpm"] for r in rows], [r[key] for r in rows], color=cmap(q), lw=1.4,
                        label=f"{q * 100:.0f} %")
            ax.axhline(0, color="k", lw=0.6)
            style(ax, "engine speed, rpm", "net torque, N·m" if col == 0 else "", f"{CARS[car]} — {title}")
        axes[1].legend(title="pedal", fontsize=7, ncol=2, loc="lower left")
        save(fig, f"torque-by-pedal-{car}.png")

    # 2. The most net torque over rpm at each pedal, as a share of MaxTorque.
    fig, ax = plt.subplots(figsize=(11, 4.6))
    shares = {}
    for car, (st, _, p) in data.items():
        pts = []
        for q in sorted({r["pedal"] for r in st}):
            rows = [r for r in st if r["pedal"] == q and r["rpm"] >= p["idle"]]
            pts.append((q * 100, 100 * max(r["torque"] for r in rows) / p["max_torque"],
                        100 * max(r["ngp"] for r in rows) / p["max_torque"]))
        shares[car] = pts
        ax.plot([a for a, _, _ in pts], [b for _, b, _ in pts], "o-", color=COLOURS[car], label=f"ours, {car}")
    first = next(iter(shares.values()))
    ax.plot([a for a, _, _ in first], [c for _, _, c in first], "s--", color=COLOURS["ngp"],
            label="NGP's formula (the same for every car)")
    ax.plot([a for a, _ in BEAMNG], [b for _, b in BEAMNG], "^:", color="k", label="BeamNG 2023 (read from its figure)")
    ax.plot([0, 100], [0, 100], color="#bbbbbb", lw=0.8, label="1:1")
    style(ax, "pedal, %", "most net torque from IdleRPM up, % of MaxTorque", "Most torque at each pedal")
    ax.set_xlim(0, 100)
    ax.legend(fontsize=8)
    save(fig, "max-torque-by-pedal.png")

    # 3. Engine braking: closed pedal, ours against NGP's line B and rF2's SkipBarber scaled by displacement.
    fig, axes = plt.subplots(1, len(data), figsize=(11, 3.8), sharey=True)
    braking = {}
    for ax, (car, (st, _, p)) in zip(axes, data.items()):
        rows = [r for r in st if r["pedal"] == 0.0]
        ax.plot([r["rpm"] for r in rows], [r["torque"] for r in rows], color=COLOURS[car], label="ours, closed pedal")
        ax.plot([r["rpm"] for r in rows], [r["ngp"] for r in rows], "--", color=COLOURS["ngp"], label="NGP line B")
        s = p["litres"] / 2.0
        ax.plot([a for a, _ in RF2], [b * s for _, b in RF2], "o", color="k",
                label=f"rF2 SkipBarber 2.0 L × {s:.2f} (displacement)")
        ax.axhline(0, color="k", lw=0.6)
        ax.axvline(p["idle"], color="#bbbbbb", lw=0.8, ls=":", label="IdleRPM (below it the idle air is wide open)")
        style(ax, "engine speed, rpm", "net torque, N·m" if ax is axes[0] else "", car)
        ax.legend(fontsize=6.5, loc="lower left")
        at = {round(r["rpm"]): r for r in rows}
        braking[car] = [(n, at[n]["torque"], at[n]["ngp"], b * s) for n, b in RF2 if n in at]
    save(fig, "engine-braking.png")

    # 4. Tip-in at fixed speed, ours against NGP's (instant); anti-lag on and off.
    panels = [("na", "tip-in 3000", 3000, 1.0), ("na", "tip-in 5000", 5000, 1.0), ("turbo", "spool 2500", 2500, 1.0),
              ("turbo", "tip-in 3000", 3000, 1.0), ("turbo", "tip-in 5000", 5000, 1.0)]
    panels = [x for x in panels if x[0] in data]
    fig, axes = plt.subplots(1, len(panels), figsize=(11, 3.6), sharey=True)
    for ax, (car, scen, rpm, at) in zip(axes, panels):
        st, tr, p = data[car]
        rows = [r for r in tr[scen] if at - 0.25 <= r["time"] <= at + 1.5]
        ax.plot([r["time"] - at for r in rows], [r["torque"] for r in rows], color=COLOURS[car], label="ours")
        ax.plot([r["time"] - at for r in rows], [ngp(p, rpm, 1.0 if r["time"] >= at else 0.0) for r in rows], "--",
                color=COLOURS["ngp"], label="NGP (instant)")
        style(ax, "time from the step, s", "net torque, N·m" if ax is axes[0] else "", f"{car}, {rpm} rpm")
        ax.legend(fontsize=7, loc="lower right")
    save(fig, "tip-in.png")
    if "turbo" in data:
        _, tr, _ = data["turbo"]
        fig, axes = plt.subplots(1, 2, figsize=(11, 3.8))
        for scen, ls, label in (("lift and re-apply 4500", "-", "anti-lag on"),
                                ("lift and re-apply 4500 no anti-lag", "--", "anti-lag off")):
            rows = tr[scen]
            t = [r["time"] for r in rows]
            axes[0].plot(t, [r["torque"] for r in rows], ls, color=COLOURS["turbo"], label=label)
            axes[1].plot(t, [r["boost"] / 1e5 for r in rows], ls, color=COLOURS["turbo"], label=label)
        for ax in axes:
            ax.axvspan(2.0, 3.0, color="#eeeeee", label="pedal lifted")
        style(axes[0], "time, s", "net torque, N·m", "turbo at 4500 rpm: lift for 1 s and re-apply")
        style(axes[1], "time, s", "boost pressure, bar abs", "boost")
        axes[0].legend(fontsize=8)
        save(fig, "anti-lag.png")

    # 5. Neutral with the car's inertia: idle, a 300 rpm dip, a full blip; ours against NGP's replica.
    cars = [c for c in ("old", "na", "turbo") if c in data]
    fig, axes = plt.subplots(len(cars), 1, figsize=(11, 2.6 * len(cars)), sharex=True)
    for ax, car in zip(axes, cars):
        _, tr, p = data[car]
        rows = tr["neutral"]
        ax.plot([r["time"] for r in rows], [r["rpm"] for r in rows], color=COLOURS[car], label="ours")
        sim = ngp_neutral(p, p["idle"], 10.0, lambda t: 1.0 if 6.0 <= t < 6.3 else 0.0, (3.0, 300.0))
        ax.plot([a for a, _, _ in sim], [b for _, b, _ in sim], "--", color=COLOURS["ngp"],
                label="NGP's formula (replica, not the bit-exact port)")
        ax.axhline(p["idle"], color="#bbbbbb", lw=0.8, label="IdleRPM")
        style(ax, "time, s" if ax is axes[-1] else "", "rpm", f"{CARS[car]}: idle, a 300 rpm dip at 3 s, a blip at 6 s")
        ax.legend(fontsize=7, loc="upper left")
    save(fig, "idle-and-rev.png")

    # 6. The limiter in neutral at full pedal: rpm and torque, ours against NGP's hard cut.
    cars = [c for c in ("na", "turbo") if c in data]
    fig, axes = plt.subplots(2, len(cars), figsize=(11, 5.6), sharex=True)
    for col, car in enumerate(cars):
        _, tr, p = data[car]
        rows = [r for r in tr["limiter"]]
        sim = ngp_neutral(p, p["idle"], 3.0, lambda t: 1.0 if t >= 0.2 else 0.0)
        axes[0][col].plot([r["time"] for r in rows], [r["rpm"] for r in rows], color=COLOURS[car], label="ours")
        axes[0][col].plot([a for a, _, _ in sim], [b for _, b, _ in sim], "--", color=COLOURS["ngp"],
                          label="NGP (replica)")
        axes[0][col].axhline(p["rev_limit"], color="#bbbbbb", lw=0.8, label="RPMLimit")
        axes[1][col].plot([r["time"] for r in rows], [r["torque"] for r in rows], color=COLOURS[car], lw=0.6,
                          label="ours")
        axes[1][col].plot([a for a, _, _ in sim], [c for _, _, c in sim], "--", color=COLOURS["ngp"], lw=0.8,
                          label="NGP (replica)")
        style(axes[0][col], "", "rpm", f"{car}: full pedal in neutral")
        style(axes[1][col], "time, s", "net torque, N·m")
        for ax in (axes[0][col], axes[1][col]):
            ax.set_xlim(0.5, 3.0)
            ax.legend(fontsize=7, loc="lower right")
        axes[0][col].set_ylim(p["rev_limit"] - 600, p["rev_limit"] + 400)
    save(fig, "limiter.png")

    for path in written:
        print(path)
    print("\nmost net torque by pedal, % of MaxTorque (ours per car; NGP):")
    for car, pts in shares.items():
        print(car, " ".join(f"{a:.0f}:{b:.0f}" for a, b, _ in pts), "| NGP", " ".join(f"{c:.0f}" for _, _, c in pts))
    print("\nclosed pedal, N·m at rpm: ours / NGP / rF2 scaled")
    for car, rows in braking.items():
        print(car, " ".join(f"{n}: {a:.1f}/{b:.1f}/{c:.1f}" for n, a, b, c in rows))


if __name__ == "__main__":
    if sys.argv[1] == "--figures":
        figures(sys.argv[2], sys.argv[3])
    else:
        per_car(sys.argv[1], dict(a.split(":") for a in sys.argv[2:]))
