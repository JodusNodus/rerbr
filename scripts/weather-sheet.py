#!/usr/bin/env python3
"""Weather reference: one view with the sky well in frame under every cloud cover, moment of the day and rain the
launcher offers, as one sheet per moment (rows the clouds, columns the rain).

    scripts/weather-sheet.py [--out DIR] [--doc DIR] [--sun MOMENT...] [--jobs N] [--missing] [--no-render]
                             [--home DIR] [--dry]

The view is the Joux Verte reference shot's (`day-joux-verte-celica-chase` in data/shots.json, its stage and car)
from 6 m higher, pitched 6 degrees up at 70 degrees of field of view, so that the clouds fill the upper half. Each
picture is a `render-shots.py` shot (the launcher's command, every enhancement on, the autopilot driving; the car still
at the start, out of the view), with the launcher's clouds, precipitation and start time set and the road dry. Writes
DIR/<moment>-<clouds>-<rain>.png and .log; `--doc DIR` also writes DIR/<moment>.jpg (each picture 480x270, labelled)
and DIR/README.md (docs/weather/ holds the committed ones). `--missing` renders only what --out lacks, `--no-render`
only makes the sheets from what it has. Needs what render-shots.py needs for that shot: the release `rerbr`, `viewer`
and `rbr-standalone`, NGP, the car, and Joux Verte's SS7 imported reversed (`AC_Joux_Verte_SS7_2020_reverse`,
notes/assetto.md).
"""
import argparse, copy, importlib.util, json, subprocess, sys, tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
spec = importlib.util.spec_from_file_location('render_shots', ROOT / 'scripts/render-shots.py')
shots = importlib.util.module_from_spec(spec)
spec.loader.exec_module(shots)

BASE = 'day-joux-verte-celica-chase'
# The drive's frame: the car still at the start, out of the view, and the clouds settled.
AT = 3000
ARGS = ['--camera', '1226.6,125.0,94.8,23.0,6,-75.3', '--fov', '70', '--sky', '41M_crisp_partcloud']
# The launcher's choices and labels (`time_of_day::label`, `settings::weather`), in its order.
SUNS = [('best', 'Best light'), ('golden', 'Golden hour'), ('midday', 'Midday'), ('dusk', 'Dusk'), ('night', 'Night')]
CLOUDS = [('clear', 'Clear'), ('dimpled', 'Dimpled clouds'), ('partly', 'Partly cloudy'), ('broken', 'Broken clouds'),
          ('heavy', 'Overcast')]
RAIN = [('none', 'no rain'), ('light', 'light rain'), ('moderate', 'moderate rain'), ('heavy', 'heavy rain')]
TILE = (480, 270)


def matrix(base, suns):
    """The shots: each moment, cover and rain over the base shot's stage and car."""
    out = []
    for sun, _ in suns:
        for clouds, _ in CLOUDS:
            for rain, _ in RAIN:
                s = {k: copy.deepcopy(base[k]) for k in ('stage', 'car')}
                s.update(name=f'{sun}-{clouds}-{rain}', view='chase', at=AT, sun=sun, args=list(ARGS),
                         settings={'clouds': clouds, 'precipitation': rain, 'start_time': sun})
                out.append(s)
    return out


def magick(*args):
    done = subprocess.run(['magick', *map(str, args)], capture_output=True, text=True)
    if done.returncode:
        sys.exit(f'magick failed: {(done.stderr or done.stdout).strip()}')


def sheet(out, sun, dest):
    """dest: the moment's pictures, rows the clouds and columns the rain, each labelled; a grey tile where one is missing."""
    tmp = Path(tempfile.mkdtemp(prefix='rerbr-weather-'))
    try:
        rows = []
        for clouds, cloud_label in CLOUDS:
            tiles = []
            for rain, rain_label in RAIN:
                png, tile = out / f'{sun}-{clouds}-{rain}.png', tmp / f'{clouds}-{rain}.jpg'
                source = [png, '-resize', f'{TILE[0]}x{TILE[1]}!'] if png.exists() else \
                    ['-size', f'{TILE[0]}x{TILE[1]}', 'xc:gray30']
                magick(*source, '-fill', 'rgba(0,0,0,0.55)', '-draw', f'rectangle 0,0 {TILE[0]},30', '-gravity',
                       'NorthWest', '-font', 'DejaVu-Sans', '-pointsize', '18', '-fill', 'white', '-annotate', '+10+5',
                       f'{cloud_label}, {rain_label}', tile)
                tiles.append(tile)
            row = tmp / f'{clouds}.jpg'
            magick(*tiles, '+append', row)
            rows.append(row)
        magick(*rows, '-append', '-strip', '-quality', '88', dest)
    finally:
        subprocess.run(['rm', '-rf', tmp])


def page(suns, dest):
    lines = ['# Weather reference\n',
             'One view with the sky well in frame (the Joux Verte reference shot\'s stage, from 6 m higher and pitched',
             '6 degrees up, 70 degrees of field of view) under every cloud cover, moment of the day and rain the launcher',
             'offers: per moment, rows are the clouds and columns the rain, the road dry, every enhancement on. Made by',
             '`scripts/weather-sheet.py`; regenerate with `scripts/weather-sheet.py --doc docs/weather` and compare before',
             'and after a change to the sky, clouds, haze, rain or the exposure.\n']
    for sun, label in suns:
        lines.append(f'## {label}\n\n![{label}]({sun}.jpg)\n')
    dest.write_text('\n'.join(lines))


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--out', default=ROOT / 'target/weather', type=Path)
    ap.add_argument('--doc', type=Path, metavar='DIR', help='write the sheets and README.md to DIR')
    ap.add_argument('--sun', nargs='+', choices=[s for s, _ in SUNS], help='only these moments')
    ap.add_argument('--build', default=ROOT / 'target/release', type=Path, help='folder with rerbr, viewer, rbr-standalone')
    ap.add_argument('--home', help="the player's files folder (default $RERBR_HOME or ~/.local/share/rerbr)")
    ap.add_argument('--size', default='1280x720', help='WIDTHxHEIGHT of each picture')
    ap.add_argument('--jobs', type=int, default=shots.JOBS)
    ap.add_argument('--no-render', action='store_true', help='only make the sheets from the PNGs in --out')
    ap.add_argument('--missing', action='store_true', help='render only the pictures --out does not have yet')
    ap.add_argument('--dry', action='store_true', help='print each command instead of running it')
    a = ap.parse_args()
    base = next(s for s in json.loads((ROOT / 'data/shots.json').read_text()) if s['name'] == BASE)
    suns = [s for s in SUNS if not a.sun or s[0] in a.sun]
    run = matrix(base, suns)
    if a.missing:
        run = [s for s in run if not (a.out / f"{s['name']}.png").exists()]
    if not a.no_render:
        done = set(shots.render(a, shots.files_folder(a.home), run))
        failed = [s['name'] for s in run if s['name'] not in done]
        if failed and not a.dry:
            print(f"failed: {', '.join(failed)}", file=sys.stderr)
    if a.doc and not a.dry:
        a.doc.mkdir(parents=True, exist_ok=True)
        for sun, _ in suns:
            sheet(a.out, sun, a.doc / f'{sun}.jpg')
        page(SUNS, a.doc / 'README.md')


if __name__ == '__main__':
    main()
