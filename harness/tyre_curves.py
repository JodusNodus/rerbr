"""tyre_curves.py: lateral force against slip angle at fixed slip ratios, NGP and the brush side by side, as SVG.

  cargo run --release -p rbr-tyre-lab --example lat_sweep -- --probe P --step N --wheel K > CURVES.csv
  python3 -I -B harness/tyre_curves.py CURVES.csv OUT.svg "TITLE"   (rsvg-convert OUT.svg -o OUT.png for a PNG)

Research, opt-in; standard library only (no matplotlib). The probes are the scorecard's (`tyre_scorecard.py` PROBES:
tarmac step 8000 wheel 1, gravel 6000 / 0, snow 6000 / 0). Curves per slip ratio κ: braking solid, driving dashed.
"""
import collections
import csv
import sys

src, out, title = sys.argv[1], sys.argv[2], sys.argv[3]
rows = [r for r in csv.DictReader(l for l in open(src) if not l.startswith("#"))]
data = collections.defaultdict(list)
for r in rows:
    data[(r["model"], float(r["kappa"]))].append((float(r["alpha_deg"]), float(r["fy_n"])))
models = ["ngp", "brush"]
names = {"ngp": "ngp (NGP's tyre)", "brush": "experimental-brush"}
kappas = sorted({k for _, k in data}, key=lambda k: (k != 0, -abs(k) if k > 0 else abs(k), k))
colors = {0.0: "#e8b300", -0.05: "#e07b39", -0.1: "#c0392b", -0.2: "#7d3c98", 0.1: "#2e86c1", 0.05: "#48c9b0"}
dash = lambda k: ' stroke-dasharray="7,5"' if k > 0 else ""
label = lambda k: "κ 0 (free rolling)" if k == 0 else (f"κ {k:+.2f} braking" if k < 0 else f"κ {k:+.2f} driving")
ymax = max(abs(y) for v in data.values() for _, y in v)
ytop = (int(ymax / 1000) + 1) * 1000
W, H, PW, PH, ML, MT = 1600, 640, 680, 470, 70, 80
svg = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" font-family="sans-serif" font-size="13">',
       f'<rect width="{W}" height="{H}" fill="white"/>',
       f'<text x="{W/2}" y="30" text-anchor="middle" font-size="20" fill="#333">{title}</text>']
for i, m in enumerate(models):
    x0 = ML + i * (PW + 110)
    sx = lambda a: x0 + (a + 40) / 80 * PW
    sy = lambda f: MT + (ytop - f) / (2 * ytop) * PH
    svg.append(f'<text x="{x0 + PW/2}" y="{MT - 14}" text-anchor="middle" font-size="16" fill="#333">{names[m]}</text>')
    for a in range(-40, 41, 10):
        svg.append(f'<line x1="{sx(a)}" y1="{MT}" x2="{sx(a)}" y2="{MT+PH}" stroke="#e3e3e3"/>')
        svg.append(f'<text x="{sx(a)}" y="{MT+PH+18}" text-anchor="middle" fill="#555">{a}</text>')
    for f in range(-ytop, ytop + 1, 1000):
        svg.append(f'<line x1="{x0}" y1="{sy(f)}" x2="{x0+PW}" y2="{sy(f)}" stroke="#e3e3e3"/>')
        svg.append(f'<text x="{x0-6}" y="{sy(f)+4}" text-anchor="end" fill="#555">{f}</text>')
    svg.append(f'<line x1="{x0}" y1="{sy(0)}" x2="{x0+PW}" y2="{sy(0)}" stroke="#666"/>')
    svg.append(f'<line x1="{sx(0)}" y1="{MT}" x2="{sx(0)}" y2="{MT+PH}" stroke="#666"/>')
    svg.append(f'<text x="{x0+PW/2}" y="{MT+PH+40}" text-anchor="middle" fill="#333">slip angle (deg)</text>')
    svg.append(f'<text transform="translate({x0-50},{MT+PH/2}) rotate(-90)" text-anchor="middle" fill="#333">'
               'lateral force Fy (N)</text>')
    for k in kappas:
        pts = data.get((m, k))
        if not pts: continue
        d = " ".join(f"{sx(a):.1f},{sy(f):.1f}" for a, f in pts)
        svg.append(f'<polyline points="{d}" fill="none" stroke="{colors.get(k, "#888")}" stroke-width="2.4"{dash(k)}/>')
lx = ML
for j, k in enumerate(kappas):
    x = lx + j * 250
    svg.append(f'<line x1="{x}" y1="{H-22}" x2="{x+30}" y2="{H-22}" stroke="{colors.get(k, "#888")}" '
               f'stroke-width="3"{dash(k)}/>')
    svg.append(f'<text x="{x+38}" y="{H-17}" fill="#333">{label(k)}</text>')
svg.append("</svg>")
open(out, "w").write("\n".join(svg))
