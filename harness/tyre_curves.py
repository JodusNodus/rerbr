"""tyre_curves.py: the tyre figures of docs/experimental-physics.md, NGP's tyre and the brush side by side, as SVG.

  python3 -I -B harness/tyre_curves.py --probes DIR [--figures docs/images/tyre-model]

DIR holds the scorecard's probe dumps (`tyre_scorecard.py`'s work folder: probe-tarmac.probe, probe-gravel.probe,
probe-snow.probe; the wheel and step are its PROBES'). Per surface, `rbr-tyre-lab`'s `force_sweep` example
(`cargo build --release -p rbr-tyre-lab --example force_sweep`) gives the steady forces at 4 kN and 15 m/s, dry:

- lat-SURFACE.svg: lateral force against the slip angle at fixed slip ratios κ (braking solid, driving dashed);
- long-SURFACE.svg: longitudinal force against the slip ratio (-1 locked, braking negative) at fixed slip angles.

Research, opt-in; standard library only (no matplotlib).
"""
import argparse
import collections
import csv
import io
import os
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tyre_scorecard  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LAB = os.path.join(REPO, "target", "release", "examples", "force_sweep")
SURFACES = {"tarmac": "Dry tarmac", "gravel": "Dry gravel (9 mm loose layer)", "snow": "Snow"}
MODELS = {"ngp": "ngp (NGP 7's tyre)", "brush": "experimental-brush"}
KAPPA_COLORS = {0.0: "#e8b300", -0.05: "#e07b39", -0.1: "#c0392b", -0.2: "#7d3c98", 0.1: "#2e86c1"}
ALPHA_COLORS = {0.0: "#e8b300", 2.0: "#e07b39", 5.0: "#c0392b", 10.0: "#7d3c98"}
W, H, PW, PH, ML, MT = 1600, 640, 680, 470, 70, 80


def sweep(probe, step, wheel, long):
    cmd = [LAB, "--probe", probe, "--step", str(step), "--wheel", str(wheel)] + (["--long"] if long else [])
    text = subprocess.run(cmd, check=True, capture_output=True, text=True).stdout
    rows = csv.DictReader(io.StringIO("".join(line + "\n" for line in text.splitlines() if not line.startswith("#"))))
    return list(rows)


def kappa_label(k):
    return "κ 0 (free rolling)" if k == 0 else (f"κ {k:+.2f} braking" if k < 0 else f"κ {k:+.2f} driving")


def plot(rows, title, long):
    """One SVG: a panel per model, a curve per fixed value (κ for lateral, α for longitudinal)."""
    xkey, skey, ykey = ("kappa", "alpha_deg", "fx_n") if long else ("alpha_deg", "kappa", "fy_n")
    data = collections.defaultdict(list)
    for r in rows:
        data[(r["model"], float(r[skey]))].append((float(r[xkey]), float(r[ykey])))
    series = sorted({s for _, s in data}, key=lambda s: (s != 0, s > 0, abs(s)))
    colors = ALPHA_COLORS if long else KAPPA_COLORS
    xlo, xhi, xstep = (-1.0, 1.0, 0.25) if long else (-40.0, 40.0, 10.0)
    ytop = (int(max(abs(y) for v in data.values() for _, y in v) / 1000) + 1) * 1000

    def dash(s):
        return ' stroke-dasharray="7,5"' if not long and s > 0 else ""

    def label(s):
        return f"slip angle {s:g}°" if long else kappa_label(s)

    svg = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" font-family="sans-serif" '
           'font-size="13">', f'<rect width="{W}" height="{H}" fill="white"/>',
           f'<text x="{W / 2}" y="30" text-anchor="middle" font-size="20" fill="#333">{title}</text>']
    for i, model in enumerate(MODELS):
        x0 = ML + i * (PW + 110)

        def sx(x):
            return x0 + (x - xlo) / (xhi - xlo) * PW

        def sy(f):
            return MT + (ytop - f) / (2 * ytop) * PH

        svg.append(f'<text x="{x0 + PW / 2}" y="{MT - 14}" text-anchor="middle" font-size="16" fill="#333">'
                   f'{MODELS[model]}</text>')
        n = round((xhi - xlo) / xstep)
        for j in range(n + 1):
            x = xlo + j * xstep
            svg.append(f'<line x1="{sx(x)}" y1="{MT}" x2="{sx(x)}" y2="{MT + PH}" stroke="#e3e3e3"/>')
            svg.append(f'<text x="{sx(x)}" y="{MT + PH + 18}" text-anchor="middle" fill="#555">{x:g}</text>')
        for f in range(-ytop, ytop + 1, 1000):
            svg.append(f'<line x1="{x0}" y1="{sy(f)}" x2="{x0 + PW}" y2="{sy(f)}" stroke="#e3e3e3"/>')
            svg.append(f'<text x="{x0 - 6}" y="{sy(f) + 4}" text-anchor="end" fill="#555">{f}</text>')
        svg.append(f'<line x1="{x0}" y1="{sy(0)}" x2="{x0 + PW}" y2="{sy(0)}" stroke="#666"/>')
        svg.append(f'<line x1="{sx(0)}" y1="{MT}" x2="{sx(0)}" y2="{MT + PH}" stroke="#666"/>')
        xname = "slip ratio κ (-1 locked, braking left)" if long else "slip angle (deg)"
        yname = "longitudinal force Fx (N)" if long else "lateral force Fy (N)"
        svg.append(f'<text x="{x0 + PW / 2}" y="{MT + PH + 40}" text-anchor="middle" fill="#333">{xname}</text>')
        svg.append(f'<text transform="translate({x0 - 50},{MT + PH / 2}) rotate(-90)" text-anchor="middle" '
                   f'fill="#333">{yname}</text>')
        for s in series:
            pts = data.get((model, s))
            if pts:
                d = " ".join(f"{sx(x):.1f},{sy(f):.1f}" for x, f in pts)
                svg.append(f'<polyline points="{d}" fill="none" stroke="{colors.get(s, "#888")}" '
                           f'stroke-width="2.4"{dash(s)}/>')
    for j, s in enumerate(series):
        x = ML + j * 250
        svg.append(f'<line x1="{x}" y1="{H - 22}" x2="{x + 30}" y2="{H - 22}" stroke="{colors.get(s, "#888")}" '
                   f'stroke-width="3"{dash(s)}/>')
        svg.append(f'<text x="{x + 38}" y="{H - 17}" fill="#333">{label(s)}</text>')
    svg.append("</svg>")
    return "\n".join(svg) + "\n"


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--probes", required=True, help="the scorecard's probe dumps")
    ap.add_argument("--figures", default=os.path.join(REPO, "docs", "images", "tyre-model"))
    args = ap.parse_args()
    if not os.path.exists(LAB):
        raise SystemExit(f"{LAB}: cargo build --release -p rbr-tyre-lab --example force_sweep")
    os.makedirs(args.figures, exist_ok=True)
    for surface, name in SURFACES.items():
        p = tyre_scorecard.PROBES[surface]
        probe = os.path.join(args.probes, f"probe-{surface}.probe")
        for long, kind, what in ((False, "lat", "Lateral force at fixed slip ratios"),
                                 (True, "long", "Longitudinal force at fixed slip angles")):
            title = f"{what}: {name}, Fiesta Rally2 wheel, 4 kN, 15 m/s"
            out = os.path.join(args.figures, f"{kind}-{surface}.svg")
            with open(out, "w") as f:
                f.write(plot(sweep(probe, p["at"], p["wheel"], long), title, long))
            print(out)


if __name__ == "__main__":
    main()
