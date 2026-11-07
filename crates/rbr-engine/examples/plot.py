"""Plots of examples/curves.rs's CSVs: python3 -I plot.py DIR [DISPLACEMENT_L=car:litres ...]. Writes DIR/<car>.png."""
import csv, sys, os, collections
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

d = sys.argv[1]
litres = dict(a.split(":") for a in sys.argv[2:])
# rF2 SkipBarber 2.0 L coast torque (report §1.1), N·m at rpm.
RF2 = [(2000, -20.8), (4000, -29.9), (6000, -40.7), (8000, -52.6)]
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
