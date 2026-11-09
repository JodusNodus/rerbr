"""implied_grip.py: the grip real rally cars use per surface, from official stage times and stage routes (research).

  python3 -I -B harness/implied_grip.py fetch --out DIR [--years 2018,2021,2022,2023,2024,2025,2026]
  python3 -I -B harness/implied_grip.py calib --route GEOJSON --telemetry TDIR [--api-dir DIR/YEAR-EVENT]
      [--surface gravel|tarmac] [--stages 7] [--explore] [--part-from KM --part-times label:wrc|rally2:s,...] --csv OUT
  python3 -I -B harness/implied_grip.py stages --data DIR --csv OUT [--class RC2|RC1] [--len-tol 0.02] [--no-sens]
  python3 -I -B harness/implied_grip.py summary --rc2 RC2.csv --rc1 RC1.csv [--calib CAL.csv...] [--export CSV]

Research, opt-in; standard library and numpy only; nothing here runs the game. Raw data stay in DIR (not in git).

`fetch` downloads WRC's live-timing JSON (the Red Bull host wrc.com's timing pages read, as RallyDataJunkie's
`wrc_rallydj/livetiming_api2.py` uses it; api.wrc.com no longer resolves): season-detail, per event `events/E.json`,
`stages.json` (stage and split distances, km), `entries.json` (car class `eventClasses`: RC1/Rally1, RC2/Rally2),
`stagetimes.json` and `splittimes.json` per stage; and RallyDataJunkie's stage routes (github.com/RallyDataJunkie/
wrc-shinylive `geodir/<event>_<year>.json`, KML LineStrings, lon/lat, no elevation; 2022 on). Before 2022 only Rally
Finland 2021 and Monte-Carlo 2018, the rallies with WRC+ telemetry (their routes come with the telemetry).

Definitions:

- Route: a LineString per stage, matched by its SS numbers ("SS 2/7 ...": stages 2 and 7); parts are joined end to
  start. Projected to a local plane (equirectangular about the mean latitude), resampled every DS m. Length check:
  route length / official length within LEN_TOL (`stages --len-tol` admits more; `summary` keeps LEN_TOL); the
  curvature is scaled by route / official length so the model runs the official length. Spacing: median and p90 of
  the vertex spacing; coarse when the median exceeds MAX_SPACING m (none is).
- Curvature κ(s): the resampled path's heading, unwrapped, smoothed by a Gaussian of SIGMA m, κ = dθ/ds. SIGMA 15 m:
  on Finland 2021 SS7 the implied μ at 15 m stays within 1.5 % when the route keeps every 2nd/3rd/4th vertex (median
  spacing 20 → 103 m), at 30 m it falls 12 % (`calib --explore`). The implied μ's level depends on SIGMA (SS7: 1.40 at
  5 m, 0.89 at 15, 0.64 at 30); the calibration uses the same SIGMA, so the level cancels in the ratios.
- Speed model (quasi-steady point mass, 4WD): v_corner = sqrt(μ_y g_eff / |κ|), g_eff = g + ClA ρ v² / (2 m);
  a forward pass a = min(μ_x g_eff e, P / (m v)) - drag - rolling - grade, a backward pass by braking
  b = μ_x g_eff e + drag + rolling + grade, e = sqrt(1 - (v² κ / μ_y g_eff)²) the friction ellipse; v ≤ VMAX.
  Start from rest, flying finish, time = Σ ds / v. μ_x = RX μ_y. Grade only where telemetry gives altitude.
- Implied μ: the μ_y (RX fixed) for which the model's time equals the real time (bisection). Per stage: the class's
  best time (`mu_best`) and the median of its five best (`mu_top5`); per split section, the median section time of
  those five against the whole-stage model cut at the split distances (`split_mu`). Direction: the route as drawn or
  reversed, whichever fits the top five's split sections better (log rms, `dir_rms`); "unknown" without splits.
  Times: status Completed, source Default (assessed times left out).
- Calibration (`calib`): per telemetry stage, per WRC drive with a merged 10 Hz file, the implied μ for that
  driver's stage time on the route (`mu_route`) and on the driver's own GPS track (`mu_gps`, `mu_gps_grade` with the
  telemetry's altitude), the measured g (`rally_gg.stats`: lateral 1 s p50/p90/p99, the g-g p95 of |lateral| over
  0.5 s means, median of the 60-90/90-120/120-150 km/h bins, braking: 16 longest events from the speed and their
  0.5 s peak), the model's speed against the driver's speed matched by position (r, rms); then the stage medians and
  route_over_<g> = median mu_route / median <g>. With --api-dir, the RC2 times on the same stage (Rally2 car).
- Cars: Rally2 (RALLY2): FIA minimum 1230 kg dry, with crew and part fuel ~1430 kg; P the wheel power averaged over
  the shift band (214 kW at the crank × 0.85 transmission × 0.92 band ≈ 170 kW); CdA 0.85 m², ClA 0.2 m², VMAX 190
  km/h. WRC 2017-21 (WRC21): 1190 kg + crew + part fuel ~1360 kg, 380 hp × 0.85 × 0.92 ≈ 218 kW, CdA 0.95, ClA 1.0,
  VMAX 200. Rally1 (RALLY1, context only): 1460 kg, 230 kW (hybrid boost averaged in), CdA 0.95, ClA 0.8. Rolling
  resistance CRR per surface. `stages` moves each SENS parameter to its low and high value (`sens_<key>`: μ at both);
  mu_rss is the root sum of squares of the largest move per car/model parameter (σ and the ellipse left out: the
  calibration uses the same choice) and, for an unknown direction, of the other direction's.
"""
import argparse
import csv
import json
import math
import os
import re
import statistics
import sys
import time
import unicodedata
import urllib.request

import numpy as np

RB = "https://p-p.redbull.com/rb-wrccom-lintegration-yv-prod/api/"
GEO = "https://raw.githubusercontent.com/RallyDataJunkie/wrc-shinylive/main/geodir/"
SEASONS = {2018: 1, 2021: 8, 2022: 11, 2023: 20, 2024: 28, 2025: 34, 2026: 47}
G = 9.80665
RHO = 1.2
DS = 5.0  # m
SIGMA = 15.0  # m
LEN_TOL = 0.01
MAX_SPACING = 60.0  # m
CRR = {"gravel": 0.025, "snow": 0.030, "tarmac": 0.013, "mixed": 0.015}
RX = 1.0  # μ_x / μ_y

RALLY2 = dict(m=1430.0, p=170e3, cda=0.85, cla=0.2, vmax_kmh=190.0)
WRC21 = dict(m=1360.0, p=218e3, cda=0.95, cla=1.0, vmax_kmh=200.0)
# Sensitivity: each entry moves one parameter to its low / high value (cars' unknowns, route, model choices).
SENS = {
    "m": (1390.0, 1480.0),
    "p": (150e3, 195e3),
    "cda": (0.7, 1.0),
    "cla": (0.0, 0.5),
    "crr": (0.6, 1.5),  # × CRR
    "sigma": (8.0, 30.0),
    "rx": (0.8, 1.2),
    "ellipse": (False, True),
}

# geodir file stem from the event's name.
GEO_NAMES = [
    ("monte", "montecarlo"), ("sweden", "sweden"), ("croatia", "croatia"), ("central europe", "cer"),
    ("japan", "japan"), ("canarias", "canarias"), ("espa", "spain"), ("spain", "spain"), ("catalunya", "spain"),
    ("finland", "finland"), ("estonia", "estonia"), ("portugal", "portugal"), ("sardegna", "sardegna"),
    ("italia", "sardegna"), ("acropolis", "greece"), ("kenya", "kenya"), ("safari", "kenya"), ("chile", "chile"),
    ("mexico", "mexico"), ("méxico", "mexico"), ("zealand", "nzl"), ("ypres", "belgium"), ("belgium", "belgium"),
    ("poland", "poland"), ("latvia", "latvia"), ("paraguay", "paraguay"), ("saudi", "saudi"),
]


# ---------------------------------------------------------------- fetch

def get(url, path, retries=3):
    if os.path.exists(path) and os.path.getsize(path) > 0:
        with open(path, "rb") as f:
            return json.loads(f.read() or b"null")
    for attempt in range(retries):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "research-script"})
            with urllib.request.urlopen(req, timeout=60) as r:
                body = r.read()
            break
        except Exception as e:  # noqa: BLE001
            if attempt == retries - 1:
                print(f"fail {url}: {e}", file=sys.stderr)
                return None
            time.sleep(2)
    with open(path, "wb") as f:
        f.write(body)
    time.sleep(0.1)
    return json.loads(body or b"null")


def geo_stem(name):
    low = name.lower()
    for key, stem in GEO_NAMES:
        if key in low:
            return stem
    return None


def fetch(args):
    out = args.out
    os.makedirs(out, exist_ok=True)
    for year in [int(y) for y in args.years.split(",")]:
        sd = get(f"{RB}season-detail.json?seasonId={SEASONS[year]}", os.path.join(out, f"season-{year}.json"))
        for rnd in sd["seasonRounds"]:
            ev = rnd["event"]
            stem = geo_stem(ev["name"])
            eid = ev["eventId"]
            # Before 2022 only the telemetry rallies (Finland 2021, Monte-Carlo 2018): their RC2 times.
            if year < 2022 and (year, stem) not in ((2021, "finland"), (2018, "montecarlo")) or "Arctic" in ev["name"]:
                continue
            d = os.path.join(out, f"{year}-{eid}")
            os.makedirs(d, exist_ok=True)
            if stem and year >= 2022:
                get(f"{GEO}{stem}_{year}.json", os.path.join(d, "route.json"))
            e = get(f"{RB}events/{eid}.json", os.path.join(d, "event.json"))
            if not e or "rallies" not in e:
                continue
            rally = next(r for r in e["rallies"] if r.get("isMain"))
            rid = rally["rallyId"]
            get(f"{RB}events/{eid}/rallies/{rid}/entries.json", os.path.join(d, "entries.json"))
            stages = get(f"{RB}events/{eid}/stages.json", os.path.join(d, "stages.json")) or []
            for st in stages:
                if st.get("stageType") != "SpecialStage":
                    continue
                sid = st["stageId"]
                get(f"{RB}events/{eid}/stages/{sid}/stagetimes.json?rallyId={rid}", os.path.join(d, f"st-{sid}.json"))
                if st.get("splitPoints"):
                    get(f"{RB}events/{eid}/stages/{sid}/splittimes.json?rallyId={rid}",
                        os.path.join(d, f"sp-{sid}.json"))
            print(year, eid, ev["name"], ev.get("surfaces"), stem, len(stages), flush=True)


# ---------------------------------------------------------------- geometry

SS_RE = re.compile(r"^\s*S{2,3}\s*[-_]?\s*((?:\d+\s*[/\-&,]\s*)*\d+)", re.I)


def ss_numbers(name):
    m = SS_RE.match(name or "")
    return [int(x) for x in re.split(r"[/\-&,]", m.group(1).replace(" ", ""))] if m else []


def xy(coords, lat0=None):
    c = np.asarray(coords, dtype=float)
    lat0 = float(np.mean(c[:, 1])) if lat0 is None else lat0
    k = math.cos(math.radians(lat0))
    return np.column_stack(((c[:, 0]) * 111320.0 * k, c[:, 1] * 110574.0)), lat0


def load_routes(path):
    """{stage number: [lon/lat polyline, ...parts]} from a geodir file."""
    with open(path) as f:
        d = json.load(f)
    d = d[0] if isinstance(d, list) else d
    out = {}
    for ft in d["features"]:
        if ft["geometry"]["type"] != "LineString":
            continue
        for n in ss_numbers(ft["properties"].get("name")):
            out.setdefault(n, []).append((ft["properties"]["name"], ft["geometry"]["coordinates"]))
    return out


def join_parts(parts):
    """One polyline from parts in file order, each turned to start nearest the last one's end."""
    line = [tuple(p[:2]) for p in parts[0][1]]
    for _, nxt in parts[1:]:
        nxt = [tuple(p[:2]) for p in nxt]
        end = np.array(line[-1])
        if np.hypot(*(np.array(nxt[-1]) - end)) < np.hypot(*(np.array(nxt[0]) - end)):
            nxt = nxt[::-1]
        line += nxt
    return line


def spacing(pts):
    d = np.hypot(*np.diff(pts, axis=0).T)
    d = d[d > 0.01]
    return d


def resample(pts, ds=DS):
    """Points every ds m along the polyline and the arc length."""
    seg = np.hypot(*np.diff(pts, axis=0).T)
    keep = np.concatenate(([True], seg > 0.01))
    pts = pts[keep]
    s = np.concatenate(([0.0], np.cumsum(np.hypot(*np.diff(pts, axis=0).T))))
    t = np.arange(0.0, s[-1], ds)
    return np.column_stack((np.interp(t, s, pts[:, 0]), np.interp(t, s, pts[:, 1]))), t


def gauss(y, sigma_n):
    """Gaussian smoothing (sigma in samples), the ends held."""
    if sigma_n <= 0:
        return y
    r = int(math.ceil(4 * sigma_n))
    k = np.exp(-0.5 * (np.arange(-r, r + 1) / sigma_n) ** 2)
    k /= k.sum()
    yp = np.concatenate((np.full(r, y[0]), y, np.full(r, y[-1])))
    return np.convolve(yp, k, mode="valid")


def curvature(pts, sigma=SIGMA, ds=DS):
    """κ per resampled point (1/m, signed) from the smoothed heading."""
    d = np.diff(pts, axis=0)
    th = np.unwrap(np.arctan2(d[:, 1], d[:, 0]))
    th = np.concatenate((th, th[-1:]))
    th = gauss(th, sigma / ds)
    return np.gradient(th, ds)


def racing_line(pts, w, iters=3000, lam=0.25):
    """A low-curvature line within ±w m of the resampled centreline: Laplacian smoothing of the points, each held to
    its own normal through the centreline and clamped to |offset| ≤ w (ends fixed). w 0 is the centreline."""
    if w <= 0:
        return pts
    d = np.gradient(pts, axis=0)
    nrm = np.column_stack((-d[:, 1], d[:, 0])) / np.maximum(np.hypot(d[:, 0], d[:, 1]), 1e-9)[:, None]
    off = np.zeros(len(pts))
    for _ in range(iters):
        q = pts + nrm * off[:, None]
        lap = np.zeros_like(q)
        lap[1:-1] = 0.5 * (q[:-2] + q[2:]) - q[1:-1]
        off = np.clip(off + lam * np.einsum("ij,ij->i", lap, nrm), -w, w)
        off[0] = off[-1] = 0.0
    return pts + nrm * off[:, None]


def path_curvature(pts, sigma, w):
    """κ along the line within ±w of the route, at the route's resampled spacing."""
    line = racing_line(pts, w)
    q, s = resample(line)
    return curvature(q, sigma), s[-1]


# ---------------------------------------------------------------- speed model

def speed_profile(kappa, mu, car, crr, rx=RX, ellipse=True, grade=None, ds=DS, v0=0.0):
    """Speed (m/s) at each point: corner limit, forward (traction, power), backward (braking)."""
    m, p, cda, cla = car["m"], car["p"], car["cda"], car["cla"]
    vmax = car["vmax_kmh"] / 3.6
    cd = 0.5 * RHO * cda / m
    cl = 0.5 * RHO * cla / m
    muy, mux = mu, mu * rx
    k = np.abs(kappa)
    den = k - muy * cl * np.ones_like(k)
    with np.errstate(divide="ignore", invalid="ignore"):
        vl = np.where(den > 1e-9, np.sqrt(muy * G / np.where(den > 1e-9, den, 1.0)), vmax)
    vl = np.minimum(vl, vmax)
    gr = np.zeros_like(k) if grade is None else grade * G
    n = len(k)
    v = vl.copy()
    v[0] = min(v[0], v0)
    kl, vl_l, gr_l = k.tolist(), vl.tolist(), gr.tolist()
    f = v.tolist()

    def grip(vv, i):
        ge = G + cl * vv * vv
        if not ellipse:
            return mux * ge
        u = vv * vv * kl[i] / (muy * ge)
        return mux * ge * math.sqrt(1.0 - u * u) if u < 1.0 else 0.0

    for i in range(n - 1):
        vv = f[i]
        a = grip(vv, i)
        if vv > 0.5:
            a = min(a, p / (m * vv))
        a -= cd * vv * vv + crr * G + gr_l[i]
        w = vv * vv + 2.0 * a * ds
        nv = math.sqrt(w) if w > 0.0 else 0.3
        f[i + 1] = min(nv, vl_l[i + 1])
    b = f[:]
    for i in range(n - 2, -1, -1):
        vv = b[i + 1]
        dec = grip(vv, i + 1) + cd * vv * vv + crr * G + gr_l[i]
        lim = math.sqrt(vv * vv + 2.0 * max(dec, 0.0) * ds)
        if b[i] > lim:
            b[i] = lim
    return np.asarray(b)


def seg_times(v, ds=DS):
    vm = 0.5 * (v[1:] + v[:-1])
    return ds / np.maximum(vm, 0.3)


def model_time(kappa, mu, car, crr, cut=None, **kw):
    """Total time, or the times of the sections [cut[i], cut[i+1]) (metres) when `cut` is given."""
    t = seg_times(speed_profile(kappa, mu, car, crr, **kw))
    if cut is None:
        return float(t.sum())
    c = np.concatenate(([0.0], np.cumsum(t)))
    idx = [min(len(c) - 1, int(round(x / DS))) for x in cut]
    return [float(c[b] - c[a]) for a, b in zip(idx, idx[1:])]


def implied_mu(target, fn, lo=0.2, hi=3.0, tol=1e-4):
    """μ with fn(μ) = target (fn falls with μ); nan when out of [lo, hi]."""
    if not (fn(hi) <= target <= fn(lo)):
        return float("nan")
    while hi - lo > tol:
        mid = 0.5 * (lo + hi)
        if fn(mid) > target:
            lo = mid
        else:
            hi = mid
    return 0.5 * (lo + hi)


# ---------------------------------------------------------------- data

def load_json(path):
    with open(path) as f:
        return json.load(f)


CLASS_ALIASES = {"RC2": {"RC2", "Rally2"}, "RC1": {"RC1", "Rally1"}}


def class_times(d, stage, klass):
    """Sorted (seconds, driver) of the class's completed default times on the stage."""
    path = os.path.join(d, f"st-{stage['stageId']}.json")
    if not os.path.exists(path):
        return []
    entries = {e["entryId"]: e for e in load_json(os.path.join(d, "entries.json"))}
    out = []
    for x in load_json(path) or []:
        e = entries.get(x["entryId"])
        if not e or x.get("status") != "Completed" or x.get("source") != "Default" or not x.get("elapsedDurationMs"):
            continue
        if CLASS_ALIASES.get(klass, {klass}) & {c["name"] for c in e.get("eventClasses") or []}:
            out.append((x["elapsedDurationMs"] / 1000.0, e["driver"]["lastName"], x["entryId"]))
    return sorted(out)


def split_times(d, stage, entry_ids):
    """{entryId: [split elapsed s in distance order]} and the split distances (km), for splits all of them have."""
    path = os.path.join(d, f"sp-{stage['stageId']}.json")
    pts = sorted(stage.get("splitPoints") or [], key=lambda p: p["distance"])
    if not os.path.exists(path) or not pts:
        return {}, []
    rows = load_json(path) or []
    by = {}
    for r in rows:
        if r.get("elapsedDurationMs"):
            by.setdefault(r["entryId"], {})[r["splitPointId"]] = r["elapsedDurationMs"] / 1000.0
    keep = [p for p in pts if all(p["splitPointId"] in by.get(e, {}) for e in entry_ids)]
    return {e: [by[e][p["splitPointId"]] for p in keep] for e in entry_ids}, [p["distance"] for p in keep]


def load_tel(path):
    """(t, lon, lat, speed m/s, altitude) sorted by time, rows with a position."""
    rows = []
    with open(path, newline="") as f:
        for r in csv.DictReader(f):
            try:
                rows.append((float(r["utx"]) / 1000.0, float(r["lon"]), float(r["lat"]), float(r["speed"]) / 3.6,
                             float(r["altitude"] or "nan")))
            except (ValueError, KeyError):
                continue
    rows = sorted(r for r in rows if r[0] > 0 and r[1] and r[2])
    return np.array(rows)


# ---------------------------------------------------------------- calibration: telemetry stages

def tel_track(a, lat0, start=None, end=None):
    """The car's path from its GPS (cut to the points nearest `start` and `end`, plane m): resampled points, arc
    length, the telemetry's speed and altitude along it."""
    p, _ = xy(a[:, 1:3], lat0)
    if start is not None:
        i = int(np.argmin(np.hypot(*(p - start).T)))
        j = int(np.argmin(np.hypot(*(p - end).T)))
        a, p = a[i:j + 1], p[i:j + 1]
        if len(a) < 2:
            return np.zeros((0, 2)), np.zeros(0), np.zeros(0), np.zeros(0)
    seg = np.hypot(*np.diff(p, axis=0).T)
    keep = np.concatenate(([True], seg > 0.3))
    a, p = a[keep], p[keep]
    s = np.concatenate(([0.0], np.cumsum(np.hypot(*np.diff(p, axis=0).T))))
    t = np.arange(0.0, s[-1], DS)
    pts = np.column_stack((np.interp(t, s, p[:, 0]), np.interp(t, s, p[:, 1])))
    return pts, t, np.interp(t, s, a[:, 3]), np.interp(t, s, a[:, 4])


def nearest(cloud, pts, chunk=400):
    """Index into `cloud` of the point nearest each of `pts`."""
    out = np.empty(len(pts), dtype=int)
    for i in range(0, len(pts), chunk):
        d = ((pts[i:i + chunk, None, :] - cloud[None, :, :]) ** 2).sum(axis=2)
        out[i:i + chunk] = np.argmin(d, axis=1)
    return out


def speed_fit(vm, vt):
    """Model against telemetry speed on the same points (km/h): rms, bias, r, and the p10 / p50 / p90 of each."""
    sel = vt > 30 / 3.6
    dv = (vm - vt)[sel] * 3.6
    q = lambda v: "/".join(f"{x:.0f}" for x in np.percentile(v[sel] * 3.6, (10, 50, 90)))  # noqa: E731
    return (f"{np.sqrt(np.mean(dv ** 2)):5.1f} {np.mean(dv):+5.1f} {np.corrcoef(vm[sel], vt[sel])[0, 1]:.3f} | "
            f"p10/50/90 model {q(vm)} telemetry {q(vt)}")


def fold(name):
    return unicodedata.normalize("NFKD", name).encode("ascii", "ignore").decode().lower()


def explore(rp, rs, pts, gps, t_med, car, crr):
    """Finland's choices: σ against the telemetry's speed, the route drawn coarser, a line within ±w."""
    ev = gps.get("Evans") or next(iter(gps.values()))
    print("sigma m | route: sum|κ|ds rad, model time s at μ 1.0 | GPS (Evans): the same")
    for sg in (5.0, 8.0, 15.0, 30.0, 60.0):
        kr, kg = curvature(rp, sg), curvature(ev[0], sg)
        print(f"{sg:5.0f} | {np.abs(kr).sum() * DS:7.0f} {model_time(kr, 1.0, car, crr):6.1f} | "
              f"{np.abs(kg).sum() * DS:7.0f} {model_time(kg, 1.0, car, crr):6.1f}")
    grid = np.linspace(0.0, 1.0, 2001)
    rg = np.column_stack([np.interp(grid, rs / rs[-1], rp[:, i]) for i in (0, 1)])
    vt = np.median([g[2][nearest(g[0], rg)] for g in gps.values()], axis=0)
    print(f"sigma m | μ (route) for the median time {t_med:.1f} s | speed against the telemetry median: rms, bias, "
          f"r (points matched by position)")
    for sg in (5.0, 8.0, 15.0, 20.0, 30.0, 45.0, 60.0):
        kk = curvature(rp, sg)
        mu = implied_mu(t_med, lambda m: model_time(kk, m, car, crr))
        vm = np.interp(grid, rs / rs[-1], speed_profile(kk, mu, car, crr))
        print(f"{sg:5.0f} | {mu:.3f} | {speed_fit(vm, vt)}")
    print("vertices kept 1/n | spacing median m | μ (route) at σ 15 / 30 / 45 m")
    for nth in (1, 2, 3, 4):
        sub = np.vstack((pts[::nth], pts[-1:]))
        q, _ = resample(sub)
        if np.hypot(*(q[0] - rp[0])) > np.hypot(*(q[-1] - rp[0])):
            q = q[::-1]
        mus = [implied_mu(t_med, lambda m: model_time(curvature(q, sg), m, car, crr)) for sg in (15.0, 30.0, 45.0)]
        print(f"1/{nth} | {np.median(spacing(sub)):.0f} | " + " / ".join(f"{m:.3f}" for m in mus))
    print("line ±w m, sigma m | length km | μ (route) | speed rms, bias, r")
    for w in (0.0, 2.0, 4.0):
        for sg in (15.0, 30.0):
            kk, length = path_curvature(rp, sg, w)
            mu = implied_mu(t_med, lambda m: model_time(kk, m, car, crr))
            vm = np.interp(grid, np.arange(len(kk)) / (len(kk) - 1), speed_profile(kk, mu, car, crr))
            print(f"{w:3.0f} {sg:4.0f} | {length / 1000:.3f} | {mu:.3f} | {speed_fit(vm, vt)}")


def calib_stage(args, nr, routes, lengths, racetimes, api_dir, car):
    """One telemetry stage: per driver (route μ, GPS μ, measured g), and the medians."""
    import rally_gg  # noqa: E402  (its stats(): the same definitions as the WRC telemetry figures)

    crr = CRR[args.surface]
    L = lengths[nr]
    parts = routes.get(nr)
    if not parts:
        return None
    pts, lat0 = xy(join_parts(parts))
    raw_sp = spacing(pts)
    rp, rs = resample(pts)
    tel = {}
    for f in sorted(os.listdir(args.telemetry)):
        m = re.match(rf"df_telemetrymergeddata_SS{nr}_(.+)\.csv$", f)
        if m and fold(m.group(1)) in racetimes:
            tel[m.group(1)] = os.path.join(args.telemetry, f)
    gps = {}
    for name, path in tel.items():
        a = load_tel(path)
        if len(a) < 100:
            continue
        full = tel_track(a, lat0)
        if np.hypot(*(rp[-1] - full[0][0])) < np.hypot(*(rp[0] - full[0][0])):
            rp = rp[::-1]
        g = tel_track(a, lat0, rp[0], rp[-1])
        if len(g[0]) < 100:
            continue
        off = (np.hypot(*(rp[0] - g[0][0])), np.hypot(*(rp[-1] - g[0][-1])))
        if max(off) < 150.0 and g[1][-1] > 0.9 * rs[-1]:
            gps[name] = g
    head = dict(stage=f"SS{nr}", km=L / 1000.0, route_km=rs[-1] / 1000.0, len_ratio=rs[-1] / L,
                spacing_med=float(np.median(raw_sp)), spacing_p90=float(np.percentile(raw_sp, 90)))
    if abs(rs[-1] / L - 1) > args.len_tol or not gps:
        print(f"SS{nr}: route {rs[-1] / 1000:.2f} km against {L / 1000:.2f}, {len(gps)} GPS tracks: skipped")
        return None
    scale = L / rs[-1]
    kr = curvature(rp, SIGMA) / scale
    if args.explore:
        explore(rp, rs, pts, gps, statistics.median(racetimes[fold(k)] for k in gps), car, crr)
    rows = []
    for key, g in gps.items():
        t = racetimes[fold(key)]
        kg = curvature(g[0], SIGMA)
        alt = g[3]
        grade = np.gradient(gauss(alt, 100.0 / DS), DS) if np.isfinite(alt).all() else None
        mu_r = implied_mu(t, lambda mu: model_time(kr, mu, car, crr))
        mu_g = implied_mu(t, lambda mu: model_time(kg, mu, car, crr))
        mu_gg = implied_mu(t, lambda mu: model_time(kg, mu, car, crr, grade=grade)) if grade is not None \
            else float("nan")
        st = rally_gg.stats(tel[key])
        grid = np.linspace(0.0, 1.0, 1001)
        rg = np.column_stack([np.interp(grid, rs / rs[-1], rp[:, i]) for i in (0, 1)])
        vt = g[2][nearest(g[0], rg)]
        vm = np.interp(grid, np.arange(len(kr)) / (len(kr) - 1), speed_profile(kr, mu_r, car, crr))
        sel = vt > 30 / 3.6
        rows.append(dict(head, driver=key, time=t, mu_route=mu_r, mu_gps=mu_g, mu_gps_grade=mu_gg,
                         v_r=float(np.corrcoef(vm[sel], vt[sel])[0, 1]),
                         v_rms_kmh=float(np.sqrt(np.mean((vm - vt)[sel] ** 2))) * 3.6,
                         lat_p50=st["lat_p50"], lat_p90=st["lat_p90"], lat_p99=st["lat_p99"],
                         gg_lat_p95=statistics.median(st[f"gg{lo}"][1] for lo in (60, 90, 120)),
                         gg_decel_p95=statistics.median(st[f"gg{lo}"][2] for lo in (60, 90, 120)),
                         brk16=st["brk_speed_16"], brk_peak16=st["brk_peak_16"], vmean=st["vmean"],
                         climb_m=float(np.sum(np.clip(np.diff(gauss(alt, 100.0 / DS)), 0, None)))))
    med = dict(head, driver="median", n=len(rows))
    for k in rows[0]:
        if k not in head and k != "driver":
            med[k] = statistics.median(r[k] for r in rows)
    for k in ("lat_p90", "gg_lat_p95", "lat_p99", "brk16", "brk_peak16"):
        med[f"route_over_{k}"] = med["mu_route"] / med[k]
    if api_dir:
        stage = next((s for s in load_json(os.path.join(api_dir, "stages.json")) if s["code"] == f"SS{nr}"), None)
        r2 = class_times(api_dir, stage, "RC2") if stage else []
        if len(r2) >= 3:
            med["rc2_best_s"] = r2[0][0]
            med["rc2_top5_s"] = statistics.median(x[0] for x in r2[:5])
            med["rc2_mu_best"] = implied_mu(r2[0][0], lambda mu: model_time(kr, mu, RALLY2, crr))
            med["rc2_mu_top5"] = implied_mu(med["rc2_top5_s"], lambda mu: model_time(kr, mu, RALLY2, crr))
    if args.part_from is not None and args.part_times:
        # Times over the part of the stage from part_from km to the finish (e.g. the game's RSF 524 against SS7).
        cut = [args.part_from * 1000.0 / scale, rs[-1]]
        for item in args.part_times.split(","):
            label, cls, t = item.split(":")
            pc = {"wrc": WRC21, "rally2": RALLY2}[cls]
            mu = implied_mu(float(t), lambda m: model_time(kr, m, pc, crr, cut=cut)[0])
            med[f"part_mu_{label}"] = mu
            print(f"SS{nr} from {args.part_from} km: {label} ({cls} car) {float(t):.1f} s -> implied μ {mu:.3f}")
    return rows, med


def calib(args):
    """Telemetry stages (`--telemetry` folder: df_telemetrymergeddata_SS<n>_<driver>.csv, df_stagedata_SS<n>.csv
    with each driver's racetime ms, df_rallydata.csv with the stage lengths) against `--route` (geojson)."""
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    lengths = {}
    with open(os.path.join(args.telemetry, "df_rallydata.csv"), newline="") as f:
        for r in csv.DictReader(f):
            lengths[int(r["name"][2:])] = float(r["distance"]) * 1000.0
    routes = load_routes(args.route)
    out, meds = [], []
    nrs = [int(x) for x in args.stages.split(",")] if args.stages else sorted(lengths)
    for nr in nrs:
        racetimes = {}
        path = os.path.join(args.telemetry, f"df_stagedata_SS{nr}.csv")
        if not os.path.exists(path):
            continue
        with open(path, newline="") as f:
            for r in csv.DictReader(f):
                if r.get("racetime"):
                    racetimes[fold(r["_record_name"])] = float(r["racetime"]) / 1000.0
        res = calib_stage(args, nr, routes, lengths, racetimes, args.api_dir, dict(WRC21))
        if not res:
            continue
        rows, med = res
        out += rows
        meds.append(med)
        print(" ".join(f"{k}={v:.3f}" if isinstance(v, float) else f"{k}={v}" for k, v in med.items()), flush=True)
    if args.csv:
        cols = []
        for r in meds + out:
            cols += [k for k in r if k not in cols]
        with open(args.csv, "w", newline="") as f:
            w = csv.DictWriter(f, fieldnames=cols)
            w.writeheader()
            for r in meds + out:
                w.writerow({k: (round(v, 4) if isinstance(v, float) else v) for k, v in r.items()})


# ---------------------------------------------------------------- stages

RALLY1 = dict(m=1460.0, p=230e3, cda=0.95, cla=0.8, vmax_kmh=200.0)
CARS = {"RC2": RALLY2, "RC1": RALLY1}


def surface_of(name, surfaces):
    low = (surfaces or "").lower()
    if "monte" in name.lower():
        return "mixed"
    if "snow" in low:
        return "snow"
    if "tarmac" in low or "asphalt" in low:
        return "tarmac"
    if "gravel" in low:
        return "gravel"
    return low or "?"


def with_params(car, crr, key, value):
    """(car, crr, kw) with one sensitivity parameter moved."""
    car = dict(car)
    kw = {}
    if key in ("m", "p", "cda", "cla"):
        car[key] = value
    elif key == "crr":
        crr = crr * value
    elif key == "rx":
        kw["rx"] = value
    elif key == "ellipse":
        kw["ellipse"] = value
    return car, crr, kw


def stage_job(job):
    """Implied μ for one stage: job = dict(event, year, surface, stage, route pts, class times, splits)."""
    pts, L = job["pts"], job["length_m"]
    rp, rs = resample(pts)
    car, crr = CARS[job["klass"]], CRR.get(job["surface"], 0.02)
    times = job["times"]
    best, top5 = times[0][0], statistics.median(t for t, _, _ in times[:5])
    # Scale the route to the official length (the route's own length is within LEN_TOL): times are over L.
    scale = L / rs[-1]
    kap = {+1: curvature(rp, SIGMA) / scale, -1: curvature(rp[::-1], SIGMA) / scale}
    cut = [0.0] + [x * 1000.0 for x in job["split_km"]] + [L]
    cut_n = [x / DS / scale * DS for x in cut]  # metres on the resampled route
    secs = None
    if job["split_km"]:
        ids = [e for _, _, e in times[:5]]
        sp = job["splits"]
        real = []
        for e in ids:
            tt = [0.0] + sp[e] + [next(t for t, _, x in times if x == e)]
            real.append(np.diff(tt))
        secs = np.median(np.array(real), axis=0)
    out = dict(job["meta"])
    out.update(best_s=best, top5_s=top5, n_class=len(times))
    fit = {}
    for dirn in (+1, -1):
        k = kap[dirn]
        mu = implied_mu(top5, lambda m: model_time(k, m, car, crr), tol=1e-3)
        rms = float("nan")
        if secs is not None and np.isfinite(mu):
            mt = np.array(model_time(k, mu, car, crr, cut=cut_n))
            rms = float(np.sqrt(np.mean(np.log(mt / secs) ** 2)))
        fit[dirn] = (mu, rms)
    if secs is not None and np.isfinite(fit[1][1]) and np.isfinite(fit[-1][1]):
        dirn = +1 if fit[1][1] <= fit[-1][1] else -1
        out["direction"] = "file" if dirn > 0 else "reversed"
        out["dir_rms"] = f"{fit[dirn][1]:.3f}/{fit[-dirn][1]:.3f}"
    else:
        dirn = +1
        out["direction"] = "unknown"
        out["dir_rms"] = ""
    k = kap[dirn]
    out["mu_best"] = implied_mu(best, lambda m: model_time(k, m, car, crr), tol=1e-3)
    out["mu_top5"] = fit[dirn][0]
    out["mu_other_dir"] = fit[-dirn][0]
    if secs is not None:
        mus = []
        for i, target in enumerate(secs):
            mus.append(implied_mu(target, lambda m: model_time(k, m, car, crr, cut=cut_n)[i], tol=1e-3))
        out["split_mu"] = "/".join(f"{m:.3f}" for m in mus)
        out["split_km"] = "/".join(f"{x:.2f}" for x in job["split_km"])
    else:
        out["split_mu"] = out["split_km"] = ""
    if job["sens"]:
        deltas = []
        for key, vals in SENS.items():
            if key == "sigma":
                res = []
                for sg in vals:
                    kk = (curvature(rp, sg) if dirn > 0 else curvature(rp[::-1], sg)) / scale
                    res.append(implied_mu(top5, lambda m: model_time(kk, m, car, crr), tol=1e-3))
            else:
                res = []
                for v in vals:
                    c2, r2, kw = with_params(car, crr, key, v)
                    res.append(implied_mu(top5, lambda m: model_time(k, m, c2, r2, **kw), tol=1e-3))
            out[f"sens_{key}"] = "/".join(f"{x:.3f}" for x in res)
            if key not in ("sigma", "ellipse"):
                deltas.append(max(abs(x - out["mu_top5"]) for x in res))
        if out["direction"] == "unknown":
            deltas.append(abs(out["mu_other_dir"] - out["mu_top5"]))
        out["mu_rss"] = float(np.sqrt(np.sum(np.square(deltas))))
    return out


def stage_jobs(args):
    jobs, rejects = [], []
    for name in sorted(os.listdir(args.data)):
        d = os.path.join(args.data, name)
        if not os.path.exists(os.path.join(d, "route.json")) or not os.path.exists(os.path.join(d, "event.json")):
            continue
        year = int(name.split("-")[0])
        ev = load_json(os.path.join(d, "event.json"))
        surface = surface_of(ev["name"], ev.get("surfaces"))
        routes = load_routes(os.path.join(d, "route.json"))
        for st in load_json(os.path.join(d, "stages.json")):
            if st.get("stageType") != "SpecialStage":
                continue
            meta = dict(year=year, event=ev["name"], surface=surface, stage=st["code"], name=st["name"],
                        km=st["distance"])
            parts = routes.get(st["number"])
            if not parts:
                rejects.append((meta, "no route"))
                continue
            line = join_parts(parts) if len(parts) > 1 else [tuple(p[:2]) for p in parts[0][1]]
            pts, _ = xy(line)
            sp = spacing(pts)
            length = float(np.sum(np.hypot(*np.diff(pts, axis=0).T)))
            meta.update(route=" + ".join(n for n, _ in parts), route_km=round(length / 1000.0, 3),
                        len_ratio=round(length / (st["distance"] * 1000.0), 4),
                        spacing_med=round(float(np.median(sp)), 1), spacing_p90=round(float(np.percentile(sp, 90)), 1))
            if abs(length / (st["distance"] * 1000.0) - 1) > args.len_tol:
                rejects.append((meta, "length"))
                continue
            if np.median(sp) > MAX_SPACING:
                rejects.append((meta, "coarse"))
                continue
            times = class_times(d, st, args.klass)
            if len(times) < 3:
                rejects.append((meta, f"{len(times)} {args.klass} times"))
                continue
            splits, kept = split_times(d, st, [e for _, _, e in times[:5]])
            sel = [i for i, x in enumerate(kept) if 0.2 < x < st["distance"] - 0.2]
            split_km = [kept[i] for i in sel]
            splits = {e: [v[i] for i in sel] for e, v in splits.items()}
            # Splits must rise with distance and stay under the stage time.
            if split_km and not all(np.all(np.diff([0.0] + splits[e] + [t]) > 0) for t, _, e in times[:5]):
                split_km, splits = [], {}
            meta.update(klass=args.klass)
            jobs.append(dict(meta=meta, pts=pts, length_m=st["distance"] * 1000.0, surface=surface,
                             klass=args.klass, times=times, splits=splits, split_km=split_km, sens=not args.no_sens))
    return jobs, rejects


def stages(args):
    import concurrent.futures

    jobs, rejects = stage_jobs(args)
    print(f"{len(jobs)} stages to run, {len(rejects)} rejected", file=sys.stderr)
    with concurrent.futures.ProcessPoolExecutor(max_workers=args.jobs) as pool:
        rows = list(pool.map(stage_job, jobs, chunksize=4))
    rows += [dict(m, reject=why) for m, why in rejects]
    cols = []
    for r in rows:
        for k in r:
            if k not in cols:
                cols.append(k)
    with open(args.csv, "w", newline="") as f:
        w = csv.DictWriter(f, fieldnames=cols)
        w.writeheader()
        for r in rows:
            w.writerow({k: (round(v, 4) if isinstance(v, float) else v) for k, v in r.items()})
    print(f"{args.csv}: {len(rows)} rows", file=sys.stderr)


# ---------------------------------------------------------------- summary

def fnum(x, k):
    try:
        return float(x[k])
    except (KeyError, TypeError, ValueError):
        return float("nan")


def summary(args):
    """Per event (stages with the length within 1 %): the median stage μ (top-5 and best), its range, the RC1 μ."""
    rows = [x for x in csv.DictReader(open(args.rc2)) if not x.get("reject")]
    rc1 = {(x["year"], x["event"], x["stage"]): x for x in csv.DictReader(open(args.rc1)) if not x.get("reject")}
    ev = {}
    for x in rows:
        if abs(fnum(x, "len_ratio") - 1) <= LEN_TOL and math.isfinite(fnum(x, "mu_top5")):
            ev.setdefault((x["surface"], x["year"], x["event"]), []).append(x)
    print("surface | year | event | stages | spacing m | RC2 top-5 μ median (min-max) | best μ | rss | RC1 μ | "
          "split μ p10-p90")
    per = {}
    for (s, y, e), v in sorted(ev.items()):
        m = [fnum(x, "mu_top5") for x in v]
        b = [fnum(x, "mu_best") for x in v]
        r1 = [fnum(rc1[k], "mu_top5") for k in ((x["year"], x["event"], x["stage"]) for x in v) if k in rc1]
        r1 = [x for x in r1 if math.isfinite(x)]
        sp = [float(z) for x in v for z in (x.get("split_mu") or "").split("/") if z and z != "nan"]
        rss = [fnum(x, "mu_rss") for x in v if math.isfinite(fnum(x, "mu_rss"))]
        per.setdefault(s, []).append(statistics.median(m))
        print(f"{s} | {y} | {e} | {len(v)} | {statistics.median(fnum(x, 'spacing_med') for x in v):.0f} | "
              f"{statistics.median(m):.3f} ({min(m):.2f}-{max(m):.2f}) | {statistics.median(b):.3f} | "
              f"{statistics.median(rss) if rss else float('nan'):.3f} | "
              f"{statistics.median(r1) if r1 else float('nan'):.3f} | "
              + (f"{np.percentile(sp, 10):.2f}-{np.percentile(sp, 90):.2f}" if sp else ""))
    for s, v in per.items():
        print(f"{s}: {len(v)} events, event medians {statistics.median(v):.3f} ({min(v):.3f}-{max(v):.3f})")
    if args.export:
        cols = ["year", "event", "surface", "stage", "name", "km", "route_km", "len_ratio", "spacing_med",
                "spacing_p90", "direction", "dir_rms", "n_class", "best_s", "top5_s", "mu_best", "mu_top5", "mu_rss",
                "sens_rx", "sens_ellipse", "sens_sigma", "split_km", "split_mu", "rc1_best_s", "rc1_top5_s",
                "rc1_mu_top5", "reject"]
        with open(args.export, "w", newline="") as f:
            w = csv.DictWriter(f, fieldnames=cols, extrasaction="ignore")
            w.writeheader()
            for x in csv.DictReader(open(args.rc2)):
                y = rc1.get((x["year"], x["event"], x["stage"]), {})
                x = dict(x, rc1_best_s=y.get("best_s", ""), rc1_top5_s=y.get("top5_s", ""),
                         rc1_mu_top5=y.get("mu_top5", ""))
                if not x.get("reject") and abs(fnum(x, "len_ratio") - 1) > LEN_TOL:
                    x["reject"] = "length 1-2 %"
                w.writerow(x)
    for path in args.calib or []:
        meds = [x for x in csv.DictReader(open(path)) if x["driver"] == "median"]
        for k in ("mu_route", "lat_p90", "gg_lat_p95", "brk16", "brk_peak16", "route_over_lat_p90",
                  "route_over_gg_lat_p95", "route_over_brk_peak16", "rc2_mu_top5", "v_r"):
            v = [fnum(x, k) for x in meds if math.isfinite(fnum(x, k))]
            if v:
                print(f"{os.path.basename(path)} {k}: median {statistics.median(v):.3f} ({min(v):.3f}-{max(v):.3f}), "
                      f"{len(v)} stages")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    f = sub.add_parser("fetch")
    f.add_argument("--out", required=True)
    f.add_argument("--years", default="2021,2022,2023,2024,2025,2026")
    c = sub.add_parser("calib")
    c.add_argument("--route", required=True, help="the rally's geojson (finland_2021, montecarlo_2018)")
    c.add_argument("--telemetry", required=True, help="the folder of df_telemetrymergeddata_SS<n>_*.csv")
    c.add_argument("--api-dir", help="the fetched event folder, for the RC2 times (2021-156)")
    c.add_argument("--stages", help="stage numbers, default all")
    c.add_argument("--surface", default="gravel")
    c.add_argument("--len-tol", type=float, default=0.02)
    c.add_argument("--explore", action="store_true")
    c.add_argument("--part-from", type=float, help="km: also the μ for --part-times over the stage from here")
    c.add_argument("--part-times", help="label:wrc|rally2:seconds,...")
    c.add_argument("--csv")
    s = sub.add_parser("stages")
    s.add_argument("--data", required=True)
    s.add_argument("--csv", required=True)
    s.add_argument("--class", dest="klass", default="RC2")
    s.add_argument("--len-tol", type=float, default=LEN_TOL)
    s.add_argument("--jobs", type=int, default=8)
    s.add_argument("--no-sens", action="store_true")
    m = sub.add_parser("summary")
    m.add_argument("--rc2", required=True)
    m.add_argument("--rc1", required=True)
    m.add_argument("--calib", nargs="*")
    m.add_argument("--export", help="the per-stage CSV (data/implied-grip-stages.csv)")
    args = ap.parse_args()
    {"fetch": fetch, "calib": calib, "stages": stages, "summary": summary}[args.cmd](args)


if __name__ == "__main__":
    main()
