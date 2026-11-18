#!/usr/bin/env python3
"""Render a converted stage with ReRBR's viewer from points along its driveline, to inspect what it looks like.

    scripts/btb-shots.py STAGE_FOLDER [--at KM,KM,...|--count N] [--out DIR] [--rerbr DIR] [--size WxH]
        [--height M] [--back M] [--pitch SLOPE] [--clock HH:MM] [--args STRING]

STAGE_FOLDER: a converted BTB stage (`rbr-ac-tracks` output). It is prepared into DIR/prepared (the maps from
`$RBR_DATA/Maps`; `RBR_DATA` from the environment or ReRBR's `dev.env`, which is read without printing it), then one
`viewer --camera --screenshot` per point: the eye --height metres over the road and --back metres behind the point,
looking along the road and down by --pitch (a slope, default 0.06). --at gives distances along the driveline in km, else --count points spread over it
(default 6). Writes DIR/<km>.png (default DIR /tmp/shots-<stage>). The pictures are game content: keep them out of Git.
"""
import argparse, math, os, re, shlex, subprocess, sys
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent  # the repository


def env(rerbr):
    """The environment with ReRBR's dev.env applied, its values never printed."""
    out = dict(os.environ)
    dev = rerbr / 'dev.env'
    if dev.is_file():
        for line in dev.read_text().splitlines():
            match = re.match(r'\s*([A-Z_][A-Z0-9_]*)=([^#]*)', line)
            if match and match[2].strip() and match[1] not in out:
                out[match[1]] = os.path.expanduser(match[2].strip())
    return out


def driveline(folder):
    """The driveline's points in the scene frame (x, height, z) and their distances."""
    points = []
    for line in (folder / 'driveline.ini').read_text(errors='replace').splitlines():
        match = re.match(r'K\d+=(.*)', line.strip())
        if match:
            v = [float(x) for x in match[1].split(',')]
            points.append(((v[0], v[2], v[1]), v[6]))
    return points


def sample(points, at):
    """Position and unit plan direction at distance `at`."""
    for (a, da), (b, db) in zip(points, points[1:]):
        if db >= at:
            t = (at - da) / max(db - da, 1e-9)
            p = tuple(a[k] + (b[k] - a[k]) * t for k in range(3))
            d = (b[0] - a[0], 0.0, b[2] - a[2])
            n = math.hypot(d[0], d[2]) or 1.0
            return p, (d[0] / n, 0.0, d[2] / n)
    raise ValueError(f'{at} m is past the driveline')


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('stage', type=Path)
    ap.add_argument('--at')
    ap.add_argument('--count', type=int, default=6)
    ap.add_argument('--out', type=Path)
    ap.add_argument('--rerbr', type=Path, default=HERE)
    ap.add_argument('--size', default='1920x1080')
    ap.add_argument('--height', type=float, default=1.6)
    ap.add_argument('--back', type=float, default=6.0)
    ap.add_argument('--pitch', type=float, default=0.06)
    ap.add_argument('--clock', default='13:00')
    ap.add_argument('--args', default='')
    args = ap.parse_args()
    stage = args.stage.resolve()
    out = args.out or Path(f'/tmp/shots-{stage.name}')
    out.mkdir(parents=True, exist_ok=True)
    environment = env(args.rerbr)
    if not environment.get('RBR_DATA'):
        sys.exit('RBR_DATA is not set (environment or ReRBR dev.env)')
    prepared = out / 'prepared'
    converter = args.rerbr / 'target/release/rbr-ac-tracks'
    subprocess.run([str(converter), 'prepare', '--track', str(stage), '--out', str(prepared)], env=environment,
                   check=True, capture_output=True)
    name = next(prepared.glob('track-*_M.trk')).with_suffix('')
    points = driveline(stage)
    length = points[-1][1]
    distances = ([float(km) * 1000 for km in args.at.split(',')] if args.at else
                 [length * (i + 0.5) / args.count for i in range(args.count)])
    viewer = args.rerbr / 'target/release/viewer'
    for at in distances:
        p, d = sample(points, max(at - args.back, 0.0))
        eye = (p[0], p[1] + args.height, p[2])
        look = (d[0], -args.pitch, d[2])
        png = out / f'{at / 1000:07.3f}.png'
        command = [str(viewer), str(name), '--camera', ','.join(f'{v:.3f}' for v in (*eye, *look)), '--size',
                   args.size, '--clock', args.clock, '--no-hud', '--screenshot', str(png), *shlex.split(args.args)]
        done = subprocess.run(command, env=environment, capture_output=True, text=True)
        (out / f'{png.stem}.log').write_text(shlex.join(command) + '\n' + done.stdout + done.stderr)
        print(png if done.returncode == 0 else f'{png}: failed, see {png.stem}.log', flush=True)


if __name__ == '__main__':
    main()
