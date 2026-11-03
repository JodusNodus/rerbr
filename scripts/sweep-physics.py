#!/usr/bin/env python3
"""Physics sweep: the autopilot drives many stage x car x condition runs headless (`rbr-standalone --autopilot`) and
the script collects what went wrong.

    scripts/sweep-physics.py [--out DIR] [--only SUBSTR...] [--jobs N] [--cars NAME...] [--list] [--dry]
                             [--sample] [--build DIR] [--rsf-stages N] [--timeout-scale F]

The matrix (`matrix()`), every run from the launcher's real arguments (`rerbr --command` in a temporary files folder,
its settings fixed as render-shots.py does):
  * every BTB stage with the Toyota GR Yaris Rally2 (RSF 107); damage `realistic` on every third, wet on every sixth;
  * `--rsf-stages` RSF stages (default 60) spread over surface and country, each with 3 of the 8 RSF cars (`CARS`)
    rotating; damage `realistic` on every third run, wet (bad weather, wet road) on every fourth;
  * the 36 retail rally stages with two NGP slot cars (`--car 2`, `--car 6`, no RSF pack: the launcher does not list
    them).
`--cars NAME...` replaces the RSF and BTB cars with these (launcher car names), `--only SUBSTR...` keeps the runs whose
name contains one of them, `--sample` is 3 BTB and 3 RSF stages for timing, `--list` prints the matrix, `--dry` the
commands.

Per run (DIR/runs/<name>/: command, stderr log, result.txt): exit code, wall time, steps, the distance reached against
the stage's driveline, finished and splits (`--result-out`), what ended the drive, the stderr lines that say the physics
stopped or declined, panicked, went NaN, a stall (under 1 m/s for over 30 s after the car first moved, from the
progress lines every 500 steps), a car far above or below the driveline. DIR/summary.json has the runs and the problems
grouped by normalized message; DIR/summary.txt the same as text.

Needs the release `rerbr` and `rbr-standalone` (`cargo build --release`), the launcher settings' install folder with
NGP and the RallySimFans stages, cars and BTB stages. The player's launcher-settings.ini is only read (install, NGP
and RSF paths); BTB stages are prepared under DIR/btb. No window, sound or wheel.
"""
import argparse, shutil, concurrent.futures, importlib.util, json, os, random, re, subprocess, sys, tempfile, time
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
_spec = importlib.util.spec_from_file_location('render_shots', ROOT / 'scripts/render-shots.py')
shots = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(shots)

# Eight varied RSF cars: Group B, WRC, Rally2, front-wheel R2, RWD historic, RWD Group B, WRC 1.6, Group A.
CARS = ['Audi quattro A2 GrpB', 'Ford Focus Mk II RS WRC 2006', 'Toyota GR Yaris Rally2', 'Peugeot 208 R2',
        'Ford Escort Mk II RS Grp4', 'Porsche 911 SC RS GrpB', 'Hyundai i20 Coupe WRC 2020',
        'Toyota Celica 2000GT(ST185) GrpA']
BTB_CAR = 'Toyota GR Yaris Rally2'
RETAIL_SLOTS = [2, 6]
RSF_STAGES = 60
# Surfaces of stages_data.json's `surface_id` and how many of the RSF stages each gets.
SURFACES = {'1': ('gravel', 0.40), '2': ('tarmac', 0.35), '3': ('snow', 0.25)}
SETTINGS = {'fog': 'off', 'precipitation': 'none', 'wear': 'new', 'tyre': 'auto', 'start_time': '14:00',
            'auto_gears': 'off'}
# Launcher arguments rbr-standalone's --autopilot mode does not take (the number of values each takes).
DROP = {'--live': 0, '--saves': 1, '--profile': 1, '--callout-distance': 1, '--marker-threshold': 1}
# The simulation runs at 720 steps a second (NGP); the progress line is every 500 steps.
STEP_HZ = 720
PROGRESS = 500
STALL_SECONDS = 30
# Fast enough for a wall-clock guess: a stage of L metres gets L / 10 m/s + 2 minutes of simulated time.
SIM_SPEED = 10.0
SIM_EXTRA = 120
UNKNOWN_LENGTH = 25000
JOBS = 10
BAD = [  # (kind, regex) over stderr
    ('physics stop', re.compile(r'declined|unbound|physics stopped|pure step stopped|stopped at step', re.I)),
    ('panic', re.compile(r'panicked|panic:|RUST_BACKTRACE|stack overflow|attempt to .* overflow|index out of bounds', re.I)),
    ('nan', re.compile(r'\bnan\b|\binf\b', re.I)),
]
PROGRESS_RE = re.compile(
    r'autopilot: step (\d+): at ([-\w.]+) of ([-\w.]+) m, ([-\w.]+) m off the driveline, ([-\w.]+) m/s .*?, '
    r'([-\w.]+) m above the driveline, at ([-\w.]+),([-\w.]+),([-\w.]+)')
FLY = 25.0  # metres above or below the driveline that count as flying or fallen


def read_settings(home):
    return shots.read_keys(home / 'launcher-settings.ini')


def make_home(base, name, keys, files):
    """A temporary files folder like the player's, with `keys` for the settings and BTB stages prepared in `base/btb`."""
    tmp = Path(tempfile.mkdtemp(prefix='sweep-', dir=base))
    for entry in files.iterdir():
        if entry.name not in ('launcher-settings.ini', 'btb', 'replays'):
            (tmp / entry.name).symlink_to(entry)
    (tmp / 'btb').symlink_to(base / 'btb')
    (tmp / 'launcher-settings.ini').write_text('[launcher]\n' + ''.join(f'{k} = {v}\n' for k, v in keys.items()))
    return tmp


def launcher(build, home):
    done = subprocess.run([build / 'rerbr', '--command'], env={**os.environ, 'RERBR_HOME': str(home)},
                          capture_output=True, text=True)
    if done.returncode:
        raise RuntimeError(f'rerbr --command: {done.stderr.strip()}')
    return done.stdout.splitlines()


def discover(build, base, files, keep):
    """(maps, cars) the launcher lists as startable: {number: (label, kind)} and {name: rsf id or None}."""
    home = make_home(base, 'list', keep, files)
    done = subprocess.run([build / 'rerbr', '--list'], env={**os.environ, 'RERBR_HOME': str(home)},
                          capture_output=True, text=True)
    maps, cars = {}, {}
    for line in done.stdout.splitlines():
        if m := re.match(r'Startable: (\d+) (.*?)(?: \((RSF|BTB)\))?$', line):
            maps[int(m[1])] = (m[2], m[3] or 'retail')
        elif m := re.match(r'Startable car: (.*?) \((?:[^()]*, RSF (\d+)|[^()]*)\)$', line):
            cars[m[1]] = int(m[2]) if m[2] else None
    return maps, cars


def slug(text):
    return re.sub(r'[^A-Za-z0-9]+', '-', text).strip('-')


def matrix(maps, cars, stages_data, args):
    """The runs: dicts with name, kind, map, car (launcher name) or slot (retail car), damage, wet, length (m)."""
    lengths = {int(s['id']): float(s['length']) for s in stages_data}
    runs = []

    def add(kind, number, car=None, slot=None, damage='safe', wet=False):
        label = maps[number][0]
        who = car or f'slot{slot}'
        runs.append({'name': f'{kind}-{number}-{slug(label)}--{slug(who)}', 'kind': kind, 'map': number, 'label': label,
                     'car': car, 'slot': slot, 'damage': damage, 'wet': wet, 'length': lengths.get(number)})

    pick = args.cars or None
    btb = sorted(n for n, (_, k) in maps.items() if k == 'BTB')
    for i, number in enumerate(btb):
        for car in pick or [BTB_CAR]:
            add('btb', number, car, damage='realistic' if i % 3 == 0 else 'safe', wet=i % 6 == 5)
    rsf = [s for s in stages_data if int(s['id']) in maps and maps[int(s['id'])][1] == 'RSF']
    rng = random.Random(2026)
    chosen = []
    for sid, (_, share) in SURFACES.items():
        buckets = defaultdict(list)
        for s in rsf:
            # 2 to 18 km: long enough to matter, short enough to finish in a night.
            if s['surface_id'] == sid and 2000 <= float(s['length']) <= 18000:
                buckets[s['short_country']].append(s)
        order = sorted(buckets, key=lambda c: (-len(buckets[c]), c))
        for c in order:
            rng.shuffle(buckets[c])
        want, got = round(args.rsf_stages * share), []
        while len(got) < want and any(buckets.values()):
            for c in order:
                if buckets[c] and len(got) < want:
                    got.append(buckets[c].pop())
        chosen += got
    chosen.sort(key=lambda s: int(s['id']))
    for i, s in enumerate(chosen):
        for j in range(3):
            n = i * 3 + j
            car = (pick or CARS)[n % len(pick or CARS)]
            add('rsf', int(s['id']), car, damage='realistic' if n % 3 == 0 else 'safe', wet=n % 4 == 3)
    for number, (_, k) in sorted(maps.items()):
        if k == 'retail':
            for j, slot in enumerate(RETAIL_SLOTS):
                add('retail', number, slot=slot, damage='realistic' if (number + j) % 3 == 0 else 'safe',
                    wet=(number + j) % 4 == 3)
    if args.sample:
        sample = [r for r in runs if r['kind'] == 'btb'][:3] + [r for r in runs if r['kind'] == 'rsf'][:3]
        return sample
    if args.only:
        runs = [r for r in runs if any(o in r['name'] for o in args.only)]
    return runs


def command_for(build, run, home_base, files, keep, base):
    """(env, argv) of the headless autopilot run for `run`, from the launcher's command."""
    keys = {**keep, **SETTINGS, 'map': str(run['map']), 'car': run['car'] or BTB_CAR, 'damage': run['damage'],
            'weather': 'bad' if run['wet'] else 'good', 'wetness': 'wet' if run['wet'] else 'dry'}
    home = make_home(base, run['name'], keys, files)
    env, argv = shots.split_command(launcher(build, home))
    live = argv.index('--live')
    # The BTB stage's prepared files are named through the temporary folder's btb link: name them in the real folder.
    argv = [a.replace(str(home / 'btb'), str(base / 'btb')) for a in argv]
    shutil.rmtree(home)
    rest, i = argv[live + 1:], 1
    out = [str(build / 'rbr-standalone')]
    while i < len(rest):
        if rest[i] in DROP:
            i += 1 + DROP[rest[i]]
            continue
        out.append(rest[i])
        i += 1
    if run['slot'] is not None:
        j = out.index('--rsf-car')
        del out[j:j + 2]
        out[out.index('--car') + 1] = str(run['slot'])
    return env, out


def steps_for(run):
    length = run['length'] or UNKNOWN_LENGTH
    return int((length / SIM_SPEED + SIM_EXTRA) * STEP_HZ)


def normalize(message):
    """A message without the numbers, paths and addresses that differ between runs."""
    message = re.sub(r'(?:/[\w.-]+){2,}', '<path>', message)
    message = re.sub(r'0x[0-9a-fA-F]+', '0xADDR', message)
    message = re.sub(r'-?\d+(\.\d+)?', 'N', message)
    return message.strip()[:200]


def analyse(run, code, wall, stdout, stderr, result):
    """The run's figures and problems from its outputs."""
    info = {'name': run['name'], 'kind': run['kind'], 'map': run['map'], 'label': run['label'], 'car': run['car'] or f"slot {run['slot']}",
            'damage': run['damage'], 'wet': run['wet'], 'exit': code, 'wall': round(wall, 1)}
    problems = []
    if code:
        problems.append(('exit code', f'exit {code}: ' + (stderr.strip().splitlines() or [''])[-1]))
    for line in stderr.splitlines():
        if line.startswith('autopilot: step') or line.startswith('autopilot:'):
            continue
        for kind, rx in BAD:
            if rx.search(line):
                problems.append((kind, line.strip()))
                break
    samples = [tuple(float(x) if x not in ('NaN', 'nan') else float('nan') for x in m.groups())
               for m in PROGRESS_RE.finditer(stderr)]
    drive = re.search(r'([\d.]+) m of driveline', stderr)
    total = float(drive[1]) if drive else None
    done = re.search(r'(\d+) steps driven, finish (not reached|step \d+), ended by (.*)', stdout)
    info.update(driveline=total, steps=int(done[1]) if done else None,
                finish=None if not done or done[2] == 'not reached' else int(done[2][5:]),
                ended=done[3] if done else None, sim_s=round(samples[-1][0] / STEP_HZ, 1) if samples else None)
    info['distance'] = samples[-1][1] if samples else None
    info['max_speed'] = max((s[4] for s in samples), default=None)
    for k, v in result.items():
        info[k] = v
    for s in samples:
        if any(x != x or abs(x) == float('inf') for x in s):
            problems.append(('nan', f'NaN in the progress at step {int(s[0])}'))
            break
    # A stall: under 1 m/s for over 30 s once the car has moved.
    moved, since = False, None
    for s in samples:
        if s[4] > 1.0:
            moved, since = True, None
        elif moved and since is None:
            since = s[0]
        if moved and since is not None and (s[0] - since) / STEP_HZ > STALL_SECONDS:
            problems.append(('stall', f'under 1 m/s for over {STALL_SECONDS} s from {s[1]:.0f} m (step {int(since)})'))
            break
    if samples and not moved:
        problems.append(('stall', 'never moved'))
    high = [s for s in samples if abs(s[5]) > FLY]
    if high:
        problems.append(('height', f'{abs(high[0][5]):.0f} m {"above" if high[0][5] > 0 else "below"} the driveline '
                                   f'at {high[0][1]:.0f} m (step {int(high[0][0])}), {len(high)} samples'))
    ended = info['ended'] or ''
    if code == 0 and done:
        if info['finish'] is None:
            reason = 'off the road' if ended.startswith('off the road') else (
                'autopilot gave up' if 'step limit' not in ended else 'step limit')
            if ended.startswith('the pure step stopped'):
                reason = 'physics stop'
            problems.append(('no finish', f'{reason}: {ended}'))
        elif not info.get('split1') or not info.get('split2'):
            problems.append(('splits', f"finished with splits {info.get('split1')} {info.get('split2')}"))
    elif code == 0:
        problems.append(('no result', 'no summary line'))
    info['problems'] = [{'kind': k, 'message': m} for k, m in problems]
    return info


def read_result(path):
    out = {}
    if path.exists():
        for line in path.read_text().splitlines():
            k, _, v = line.partition('=')
            out[{'finished': 'finished', 'time': 'time', 'split1': 'split1', 'split2': 'split2'}.get(k, k)] = (
                v if k == 'cause' else float(v) if '.' in v else int(v))
    return out


def execute(run, args, build, files, keep, base, outdir):
    folder = outdir / 'runs' / run['name']
    folder.mkdir(parents=True, exist_ok=True)
    try:
        env, cmd = command_for(build, run, None, files, keep, base)
    except Exception as e:
        info = {'name': run['name'], 'kind': run['kind'], 'map': run['map'], 'label': run['label'],
                'car': run['car'] or f"slot {run['slot']}", 'exit': None,
                'problems': [{'kind': 'launcher', 'message': str(e)}]}
        (folder / 'info.json').write_text(json.dumps(info, indent=1))
        return info
    result = folder / 'result.txt'
    cmd += ['--autopilot', '/dev/null', '--steps', str(steps_for(run)), '--result-out', str(result)]
    (folder / 'command.txt').write_text('\n'.join(cmd) + '\n')
    if args.dry:
        print(' '.join(cmd))
        return None
    timeout = args.timeout_scale * ((run['length'] or UNKNOWN_LENGTH) / SIM_SPEED + SIM_EXTRA) * 4
    started = time.time()
    try:
        done = subprocess.run(cmd, env=env, capture_output=True, text=True, timeout=timeout)
        code, out, err = done.returncode, done.stdout, done.stderr
    except subprocess.TimeoutExpired as e:
        code, out, err = None, '', (e.stderr.decode() if isinstance(e.stderr, bytes) else e.stderr or '')
    wall = time.time() - started
    (folder / 'stderr.log').write_text(err)
    (folder / 'stdout.log').write_text(out)
    info = analyse(run, code, wall, out, err, read_result(result))
    if code is None:
        info['problems'].append({'kind': 'timeout', 'message': f'no end after {timeout:.0f} s'})
    (folder / 'info.json').write_text(json.dumps(info, indent=1))
    flag = ' '.join(sorted({p['kind'] for p in info['problems']})) or 'ok'
    print(f"{run['name']}: {wall:.0f}s finish={info.get('finish')} dist={info.get('distance')}/{info.get('driveline')} {flag}",
          flush=True)
    return info


def summarize(infos, outdir):
    groups = defaultdict(lambda: {'count': 0, 'runs': []})
    for info in infos:
        for p in info['problems']:
            g = groups[(p['kind'], normalize(p['message']))]
            g['count'] += 1
            g['runs'].append(info['name'])
            g.setdefault('example', p['message'])
    problems = [{'kind': k, 'message': m, **g, 'runs': sorted(set(g['runs']))[:30]} for (k, m), g in groups.items()]
    problems.sort(key=lambda p: (-p['count'], p['kind']))
    finished = [i for i in infos if i.get('finish') is not None]
    totals = {'runs': len(infos), 'finished': len(finished),
              'with_problems': sum(1 for i in infos if i['problems']),
              'wall_seconds': round(sum(i.get('wall', 0) for i in infos), 1)}
    (outdir / 'summary.json').write_text(json.dumps({'totals': totals, 'problems': problems, 'runs': infos}, indent=1))
    lines = [f"{totals['runs']} runs, {totals['finished']} finished, {totals['with_problems']} with problems, "
             f"{totals['wall_seconds']} s of run time"]
    for p in problems:
        lines.append(f"\n[{p['kind']}] x{p['count']}: {p['message']}\n  e.g. {p['example']}\n  runs: {', '.join(p['runs'][:8])}")
    (outdir / 'summary.txt').write_text('\n'.join(lines) + '\n')
    print('\n'.join(lines))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--out', type=Path)
    ap.add_argument('--only', nargs='+')
    ap.add_argument('--jobs', type=int, default=JOBS)
    ap.add_argument('--cars', nargs='+')
    ap.add_argument('--list', action='store_true')
    ap.add_argument('--dry', action='store_true')
    ap.add_argument('--sample', action='store_true')
    ap.add_argument('--build', type=Path, default=ROOT / 'target/release')
    ap.add_argument('--home', type=Path)
    ap.add_argument('--rsf-stages', type=int, default=RSF_STAGES)
    ap.add_argument('--timeout-scale', type=float, default=1.0)
    args = ap.parse_args()
    files = shots.files_folder(args.home)
    keep = {k: v for k, v in read_settings(files).items() if k in shots.KEEP}
    outdir = args.out or Path(os.environ.get('RBR_RUNS', Path.home() / 'rerbr-dev/runs')) / 'sweep-physics-2026-11-04'
    outdir.mkdir(parents=True, exist_ok=True)
    base = outdir / 'tmp'
    (base / 'btb').mkdir(parents=True, exist_ok=True)
    maps, cars = discover(args.build, base, files, keep)
    stages = json.loads((Path(keep['install_folder']) / 'rsfdata/cache/stages_data.json').read_text())
    for name in args.cars or []:
        if name not in cars:
            sys.exit(f'{name}: not a startable car')
    runs = matrix(maps, cars, stages, args)
    if args.list:
        for r in runs:
            print(r['name'], r['damage'], 'wet' if r['wet'] else 'dry', r['length'])
        print(len(runs), 'runs')
        return
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        infos = [i for i in pool.map(lambda r: execute(r, args, args.build, files, keep, base, outdir), runs) if i]
    if not args.dry:
        summarize(infos, outdir)


if __name__ == '__main__':
    main()
