#!/usr/bin/env python3
"""Load sweep: starts every RallySimFans stage and every BTB stage of the install the way the launcher does, one
offscreen autopilot run each, and collects what the program reports as a problem.

    scripts/sweep-load.py [--out DIR] [--only WORD...] [--jobs N] [--timeout S] [--at FRAME] [--car NAME]
                          [--retail-car NAME [--retail-every N]] [--kind rsf|btb|all] [--list] [--dry] [--resume]
                          [--summarize]

Stages: the `<id>-<name>` folders of `<install>/Maps`, and the BTB stages with the map numbers `rerbr --list` prints
("Startable: <number> <name> (BTB)"; the launcher's catalog numbers them, a prepared folder is made on Start). Each is
the launcher's Start command for the stage and --car (render-shots.py's `launcher_command`: its settings in a
temporary files folder, every graphics option on, no `--upscale-textures`: the upscaler takes hours on a stage not
cached), with `--screenshot` at drive frame --at, 1280x720, `--no-sound`, the wheel's force feedback off and
`--pilot autopilot`, so nothing needs a display, a wheel or sound. --retail-car also runs every --retail-every-th
stage (default 12) with that car. Every stage is killed after --timeout seconds (default 240).
--only takes words; a stage runs when one is its id or in its name (case-insensitive).

Writes OUT/<stage>.log (the command and the output), OUT/<stage>.png and OUT/summary.json: the stderr/stdout lines
that look like problems grouped by their message with numbers and paths stripped (count, stages, an example), the
crashes, timeouts, exit codes, stages without a picture and pictures that are mostly black or mostly one colour.
`--resume` skips the runs already logged; `--summarize` rebuilds summary.json from the logs already in OUT. OUT
defaults to $RBR_RUNS/sweep-load-<today>. The maps the launcher lists as unavailable (encrypted) are logged as such,
not run.

Needs the release rerbr, viewer and rbr-standalone (`cargo build --release`), the RSF install (RBR_RSF or the
launcher settings' install folder) with NGP and the car, and ImageMagick (`magick`) for the picture checks.
"""
import argparse, importlib.util, json, os, re, shlex, shutil, subprocess, sys, time
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
_spec = importlib.util.spec_from_file_location('render_shots', ROOT / 'scripts/render-shots.py')
shots = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(shots)

CAR = 'Toyota GR Yaris Rally2'
SIZE = '1280x720'
AT = 1500
TIMEOUT = 240
# Map number to the launcher's reason, for the stages it lists as unavailable.
UNAVAILABLE = {}
JOBS = 3
# Lines that name a problem: the word, as a whole word or a prefix of one.
PROBLEM = re.compile(r'\b(fail\w*|missing|unreadable|cannot|can\'t|could not|couldn\'t|no such|not found|skipp\w*|'
                     r'error\w*|panick?\w*|warn\w*|unsupported|invalid|unknown|ignor\w*|dropp\w*|without|'
                     r'unavailable|declined|no [A-Za-z]+\S*)\b', re.I)
# Lines that look like problems and are the program's ordinary report.
BENIGN = re.compile(r'^(skipped \S+: |WARNING: radv is not a conformant|(wgpu_hal|naga|winit|zbus|ALSA)\b)')
# A count of zero in a report line (`0 missing textures`, `skipped draws: 0`, `failed {}`) is not a problem.
ZERO = re.compile(r'\b0 (?:missing|bad|declined|skipped|failed)\w*(?: \w+)?|\b(?:missing|bad|skipped|failed)\w*(?: \w+)?[: ]+(?:0\b|\{\})|\b\d+ skipped\b|unmodelled calls \{\}')


def install():
    keys = shots.read_keys(shots.files_folder(None) / 'launcher-settings.ini')
    return Path(os.environ.get('RBR_RSF') or keys.get('rsf_root') or keys['install_folder'])


def stages(kind, root, only):
    """[(name, map number, kind)] of the RSF folders and the launcher's BTB maps."""
    out = []
    listing = subprocess.run([ROOT / 'target/release/rerbr', '--list'], capture_output=True, text=True).stdout
    # What the launcher lists as unavailable (RallySimFans encrypts some stages): it refuses them, no bug.
    for m in re.finditer(r'^Unavailable: (\d+) .* — (.+)$', listing, re.M):
        UNAVAILABLE[int(m[1])] = m[2]
    if kind in ('rsf', 'all'):
        for folder in sorted((root / 'Maps').iterdir()):
            m = re.match(r'(\d+)-(.+)$', folder.name)
            if m and folder.is_dir():
                out.append((folder.name.replace(' ', '_'), int(m[1]), 'rsf'))
    if kind in ('btb', 'all'):
        for m in re.finditer(r"^Startable: (\d+) (.+) \(BTB\)$", listing, re.M):
            out.append((f'btb-{m[1]}-{m[2]}'.replace(' ', '_').replace('/', '_'), int(m[1]), 'btb'))
    if only:
        words = [w.lower() for w in only]
        out = [s for s in out if any(w == str(s[1]) or w in s[0].lower() for w in words)]
    return out


def command(build, home, name, number, car, size, at):
    """(temporary files folder, environment, viewer command) of the launcher's Start for `number`."""
    shot = {'name': name, 'stage': number, 'car': car, 'view': 'chase', 'at': at,
            'settings': {'upscale_textures': 'off', 'fps': 'off', 'volume': '0'}}
    tmp, lines = shots.launcher_command(build, home, shot, size)
    # The viewer's `--bindings` file: no force on a wheel (on by default).
    with open(tmp / 'launcher-settings.ini', 'a') as ini:
        ini.write('\n[ffb]\nenabled = 0\n')
    env, cmd = shots.shot_command(build, lines, shot, tmp / 'shot.png', size)
    cmd = [c for c in cmd if c != '--upscale-textures']
    if '--no-sound' not in cmd:
        cmd.insert(1, '--no-sound')
    env['RERBR_HOME'] = str(tmp)
    return tmp, env, cmd


def run_one(a, home, name, number, kind, car, tag):
    log, png = a.out / f'{name}{tag}.log', a.out / f'{name}{tag}.png'
    if a.resume and log.exists():
        return None
    if number in UNAVAILABLE:
        log.write_text(f'# {name}{tag} map {number} {kind} car {car}\n# status unavailable after 0 s\n\n'
                       f'--- stderr\nunavailable: {UNAVAILABLE[number]}\n')
        print(f'{name}{tag}: unavailable: {UNAVAILABLE[number]}', flush=True)
        return 'unavailable'
    try:
        tmp, env, cmd = command(a.build, home, name, number, car, a.size, a.at)
    except SystemExit as e:  # render-shots.py's launcher_command exits with the launcher's reason
        log.write_text(f'# {name}{tag} map {number} {kind} car {car}\n# status launcher refused after 0 s\n\n'
                       f'--- stderr\n{e}\n')
        print(f'{name}{tag}: launcher refused: {e}', flush=True)
        return 'launcher refused'
    cmd[cmd.index('--screenshot') + 1] = str(png)
    if '--bindings' not in cmd:
        cmd[1:1] = ['--bindings', str(tmp / 'launcher-settings.ini')]
    env.pop('MANGOHUD', None)
    head = f'# {name}{tag} map {number} {kind} car {car}\n# {shlex.join(cmd)}\n'
    if a.dry:
        print(head)
        shutil.rmtree(tmp, ignore_errors=True)
        return None
    began, status = time.time(), 'ok'
    try:
        done = subprocess.run(cmd, env=env, capture_output=True, text=True, timeout=a.timeout, errors='replace')
        code, out, err = done.returncode, done.stdout, done.stderr
        if code:
            status = f'exit {code}' if code > 0 else f'signal {-code}'
    except subprocess.TimeoutExpired as e:
        code, status = None, 'timeout'
        out, err = (e.stdout or b'').decode(errors='replace'), (e.stderr or b'').decode(errors='replace')
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    seconds = time.time() - began
    log.write_text(f'{head}# status {status} after {seconds:.0f} s\n\n--- stderr\n{err}\n--- stdout\n{out}')
    print(f'{name}{tag}: {status} {seconds:.0f} s', flush=True)
    return status


def picture(png):
    """'' for a picture with variety, else why not: missing, black or one colour (ImageMagick's statistics)."""
    if not png.exists():
        return 'no picture'
    done = subprocess.run(['magick', png, '-colorspace', 'sRGB', '-format', '%[fx:mean] %[fx:standard_deviation]', 'info:'],
                          capture_output=True, text=True)
    try:
        mean, dev = map(float, done.stdout.split())
    except ValueError:
        return 'unreadable picture'
    if mean < 0.03:
        return f'mostly black (mean {mean:.3f})'
    if dev < 0.03:
        return f'mostly one colour (deviation {dev:.3f})'
    return ''


def normalize(line):
    """The line with its paths, numbers and hex stripped, so the same message from every stage groups."""
    line = re.sub(r'(/[\w .+()\-\[\]]+)+(\.\w+)?', '<path>', line)
    line = re.sub(r'\b0x[0-9a-fA-F]+\b', '<hex>', line)
    line = re.sub(r'\d+(\.\d+)?', '<n>', line)
    return re.sub(r'\s+', ' ', line).strip()[:200]


def summarize(out):
    groups, runs = defaultdict(lambda: {'count': 0, 'stages': set(), 'example': ''}), []
    for log in sorted(out.glob('*.log')):
        text = log.read_text(errors='replace')
        name = log.stem
        status = re.search(r'^# status (.+?) after', text, re.M)
        status = status[1] if status else 'unknown'
        body = text.split('\n\n', 1)[-1]
        for line in body.splitlines():
            if line.startswith('---') or not line.strip() or not PROBLEM.search(ZERO.sub('', line)) or BENIGN.search(line):
                continue
            key = normalize(line)
            g = groups[key]
            g['count'] += 1
            g['stages'].add(name)
            g['example'] = g['example'] or line.strip()[:300]
        if status == 'unavailable':
            groups.pop(normalize(body.splitlines()[-1]), None)
        runs.append({'stage': name, 'status': status, 'picture': '' if status == 'unavailable' else picture(out / f'{name}.png')})
    problems = sorted(({'message': k, 'count': g['count'], 'stage_count': len(g['stages']),
                        'stages': sorted(g['stages'])[:8], 'example': g['example']} for k, g in groups.items()),
                      key=lambda p: -p['stage_count'])
    unavailable = [r['stage'] for r in runs if r['status'] == 'unavailable']
    bad = [r for r in runs if r['status'] not in ('ok', 'unavailable') or (r['status'] == 'ok' and r['picture'])]
    summary = {'runs': len(runs), 'unavailable': unavailable, 'bad_runs': bad, 'problems': problems}
    (out / 'summary.json').write_text(json.dumps(summary, indent=1) + '\n')
    print(f"{len(runs)} runs, {len(bad)} crashed, timed out or with a bad picture, {len(problems)} distinct messages")
    return summary


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--out', type=Path)
    ap.add_argument('--build', default=ROOT / 'target/release', type=Path)
    ap.add_argument('--home', help="the player's files folder (default $RERBR_HOME or ~/.local/share/rerbr)")
    ap.add_argument('--only', nargs='*', default=[], metavar='WORD')
    ap.add_argument('--kind', choices=('rsf', 'btb', 'all'), default='all')
    ap.add_argument('--jobs', type=int, default=JOBS)
    ap.add_argument('--timeout', type=int, default=TIMEOUT)
    ap.add_argument('--at', type=int, default=AT, help='the drive frame of the screenshot')
    ap.add_argument('--size', default=SIZE)
    ap.add_argument('--car', default=CAR)
    ap.add_argument('--retail-car')
    ap.add_argument('--retail-every', type=int, default=12)
    ap.add_argument('--list', action='store_true')
    ap.add_argument('--dry', action='store_true')
    ap.add_argument('--resume', action='store_true', help='skip the runs whose log is already in --out')
    ap.add_argument('--summarize', action='store_true')
    a = ap.parse_args()
    a.out = a.out or Path(os.environ['RBR_RUNS']) / f'sweep-load-{time.strftime("%Y-%m-%d")}'
    a.out.mkdir(parents=True, exist_ok=True)
    if a.summarize:
        return summarize(a.out)
    todo = stages(a.kind, install(), a.only)
    if a.list:
        for name, number, kind in todo:
            print(f'{number:>8} {kind} {name}')
        return
    home = shots.files_folder(a.home)
    jobs = [(n, num, k, a.car, '') for n, num, k in todo]
    if a.retail_car:
        jobs += [(n, num, k, a.retail_car, '--retail') for n, num, k in todo[::a.retail_every]]
    began = time.time()
    with ThreadPoolExecutor(a.jobs) as pool:
        list(pool.map(lambda j: run_one(a, home, j[0], j[1], j[2], j[3], j[4]), jobs))
    print(f'{len(jobs)} runs in {time.time() - began:.0f} s')
    if not a.dry:
        summarize(a.out)


if __name__ == '__main__':
    main()
