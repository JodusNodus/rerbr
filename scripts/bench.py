#!/usr/bin/env python3
"""Benchmark: the autopilot drives the worst case we measured (SCENE) live in a window, every graphics option at its
most and the frame rate uncapped, timed by the viewer's performance log from the race's go for --seconds.

    scripts/bench.py [--out DIR] [--runs N] [--seconds S] [--size WxH] [--build DIR] [--home DIR] [--args STRING]
                     [--dry] [--compare OLD_DIR]

Each run is the launcher's Start command for SCENE, its settings fixed in a temporary files folder as render-shots.py
does and the wheel's force feedback off, with `--view`, `--perf-log`, `--quit-after go+S`, the replay saved in the
run's folder and `rbr-standalone --pilot autopilot`. Fullscreen at the display's size unless --size asks for a window.
Prints each run and the median over the runs: average fps, the 1% and 0.1% lows (fps at the 99th and 99.9th
percentile frame time), frame time p50, p99 and largest, hitches (`rbr_render::perf_log`), the pacing (the mean change
of the frame time from one frame to the next), the latency (`RBR_LATENCY`: ms from an input at any moment until a frame
that read it is on screen, p50 and p95; Wayland's presentation feedback, so `-` elsewhere), the GPU's frame time (below
the frame time when the CPU is the limit), the costliest GPU passes and CPU spans, and how far the autopilot drove.
MangoHud stays off.
Writes DIR/run-<n>/ (the perf log, its .trace.json of the last 600 frames for ui.perfetto.dev, the replay, the
viewer's output) and DIR/result.json; `--compare OLD_DIR` prints this result against an earlier one.

Needs the release rerbr, viewer and rbr-standalone (`cargo build --release`), a display, and the game install with
NGP, the RallySimFans stage and car (the launcher settings' install folder and NGP paths, as render-shots.py reads
them). The window takes the screen for each run: leave the machine alone while it runs.
"""
import argparse, importlib.util, json, os, re, shlex, statistics, subprocess, sys, time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
_spec = importlib.util.spec_from_file_location('render_shots', ROOT / 'scripts/render-shots.py')
shots = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(shots)

# The worst case measured (cachy's RX 9060 XT at 4K, 2026-11-02; `notes/renderer.md` "Benchmark"): heavy rain on
# Gabiria, its spectators in view, from the costliest cockpit.
SCENE = {
    'name': 'bench',
    'stage': 450,
    'car': 'Fiat 131 Abarth Grp4',
    'view': 'cockpit',
    'settings': {'weather': 'bad', 'wetness': 'wet', 'clouds': 'heavy', 'precipitation': 'heavy',
                 'start_time': '10:00'},
}
# Every graphics option at its most, the frame rate uncapped, silent; the launcher's keys over render-shots' SETTINGS.
MOST = {
    'detail': 'ultra', 'wet_roads': 'high', 'mirrors': '1', 'render_scale': '100', 'upscale_textures': 'on',
    'lighting': 'on', 'particles': 'on', 'surfaces': 'on', 'cabin': 'on', 'contact': 'on', 'grass': 'on',
    'windscreen': 'on', 'marks': 'on', 'dials': 'on', 'hud': 'on', 'fps': 'off', 'present': 'immediate',
    'perf_log': 'off', 'volume': '0',
}
# The launcher's viewer arguments the benchmark sets itself (the number of values each takes).
DROP = {'--perf-log': 1, '--save-replay': 1, '--quit-after': 1, '--view': 1}
RUNS = 3
SECONDS = 90
# Lines of the slowest GPU passes and CPU spans printed.
TOP = 8


def bench_command(build, lines, run_dir, seconds, extra):
    """(environment, viewer command) from the launcher's lines: its viewer arguments with the benchmark's, then the
    live process's, driven by the autopilot."""
    env, argv = shots.split_command(lines)
    live = argv.index('--live')
    viewer, rest = argv[:live], argv[live:]
    command, j = [str(build / 'viewer')], 1
    while j < len(viewer):
        if viewer[j] in DROP:
            j += 1 + DROP[viewer[j]]
            continue
        command.append(viewer[j])
        j += 1
    command += ['--view', SCENE['view'], '--perf-log', str(run_dir), '--quit-after', f'go+{seconds}',
                '--save-replay', str(run_dir / 'bench.rrp')] + extra
    rest[1] = str(build / 'rbr-standalone')
    return env, command + rest + ['--pilot', 'autopilot']


def parse_log(path):
    """The perf log's header, the summary from the go and its tables, and the hitches after the go."""
    text = path.read_text()
    header = {}
    for line in text.split('\n\n', 1)[0].splitlines()[1:]:
        key, _, value = line.partition('  ')
        header[key.strip()] = value.strip()
    found = re.search(r'^Summary from go \(t=([\d.]+)s\): (\d+) frames in ([\d.]+) s$', text, re.M)
    if not found:
        return None
    go, frames, seconds = float(found[1]), int(found[2]), float(found[3])
    tables, title = {}, None
    for line in text[found.end():].splitlines():
        words = line.split()
        if words[-6:] == ['n', 'mean', 'p50', 'p99', 'p99.9', 'max']:
            title = words[0]
            tables[title] = {}
        elif title and len(words) > 6:
            n, *ms = words[-6:]
            stats = dict(zip(('mean', 'p50', 'p99', 'p999', 'max'), map(float, ms)))
            tables[title][' '.join(words[:-6])] = {'n': int(n), **stats}
    hitches = sum(int(m[2]) for m in re.finditer(r'^t=(\d+)s frames \d+ frame .*? hitches (\d+)', text, re.M)
                  if int(m[1]) > go)
    return {'header': header, 'go': go, 'frames': frames, 'seconds': seconds, 'hitches': hitches, **tables}


def figures(log, output):
    """A run's numbers: frame rate, times and pacing, the latency, GPU frame, passes, spans, and the autopilot's last
    distance."""
    frame = log['frame']['frame time']
    gpu = log.get('gpu', {})
    drove = re.findall(r'^pilot: step \d+: at ([\d.]+) of ([\d.]+) m', output, re.M)
    latency = re.search(r'any input -> on screen +n +\d+ +p50 +([\d.]+) +p95 +([\d.]+)', output)
    return {
        'fps': log['frames'] / log['seconds'],
        'low1': 1000 / frame['p99'],
        'low01': 1000 / frame['p999'],
        'p50': frame['p50'], 'p99': frame['p99'], 'max': frame['max'],
        'hitches': log['hitches'],
        'pace': log['frame']['frame change']['mean'],
        'lat50': float(latency[1]) if latency else None,
        'lat95': float(latency[2]) if latency else None,
        'gpu_frame': gpu.get('GPU frame', {}).get('mean'),
        'gpu': {k: v['mean'] for k, v in gpu.items() if k != 'GPU frame'},
        'cpu': {k: v['mean'] for k, v in log.get('cpu', {}).items()},
        'drove': [float(x) for x in drove[-1]] if drove else None,
    }


COLUMNS = [('fps', 'avg fps', '{:.1f}'), ('low1', '1% low', '{:.1f}'), ('low01', '0.1% low', '{:.1f}'),
           ('p50', 'p50 ms', '{:.2f}'), ('p99', 'p99 ms', '{:.2f}'), ('max', 'max ms', '{:.1f}'),
           ('hitches', 'hitches', '{:.0f}'), ('pace', 'pace ms', '{:.2f}'), ('lat50', 'lat p50', '{:.1f}'),
           ('lat95', 'lat p95', '{:.1f}'), ('gpu_frame', 'GPU ms', '{:.2f}')]


def row(label, f):
    cells = [fmt.format(f[k]) if f.get(k) is not None else '-' for k, _, fmt in COLUMNS]
    return f'{label:8}' + ''.join(f'{c:>10}' for c in cells)


def median(runs):
    """The median of each figure over the runs; the passes' and spans' medians of their means."""
    out = {k: statistics.median(r[k] for r in runs) if all(r[k] is not None for r in runs) else None
           for k, _, _ in COLUMNS}
    for part in ('gpu', 'cpu'):
        names = {n for r in runs for n in r[part]}
        out[part] = {n: statistics.median(r[part].get(n, 0.0) for r in runs) for n in names}
    return out


def print_result(result):
    print(f"\n{'':8}" + ''.join(f'{title:>10}' for _, title, _ in COLUMNS))
    for i, run in enumerate(result['runs'], 1):
        print(row(f'run {i}', run))
    print(row('median', result['median']))
    for part, title in (('gpu', 'GPU passes'), ('cpu', 'CPU spans')):
        top = sorted(result['median'][part].items(), key=lambda kv: -kv[1])[:TOP]
        print(f"\n{title}, ms a frame (median of the runs' means):\n  " + '; '.join(f'{n} {ms:.2f}' for n, ms in top))


def compare(new, old):
    a, b = old['median'], new['median']
    print(f"\n{'':10}{'before':>10}{'after':>10}{'change':>10}")
    for k, title, fmt in COLUMNS:
        if a.get(k) is None or b.get(k) is None:
            continue
        change = f'{(b[k] / a[k] - 1) * 100:+.1f} %' if a[k] else '-'
        print(f'{title:10}{fmt.format(a[k]):>10}{fmt.format(b[k]):>10}{change:>10}')
    for part, title in (('gpu', 'GPU passes'), ('cpu', 'CPU spans')):
        names = sorted(set(a[part]) | set(b[part]), key=lambda n: -max(a[part].get(n, 0), b[part].get(n, 0)))[:TOP]
        print(f'\n{title}, ms:')
        for n in names:
            print(f'  {n:34}{a[part].get(n, 0):>8.2f}{b[part].get(n, 0):>8.2f}{b[part].get(n, 0) - a[part].get(n, 0):>+8.2f}')


def run_once(a, home, run_dir, extra):
    """One run into `run_dir`: its figures, or exits with the viewer's output."""
    run_dir.mkdir(parents=True, exist_ok=True)
    tmp, lines = shots.launcher_command(a.build, home, scene(a), a.size or '1920x1080')
    try:
        env, command = bench_command(a.build, lines, run_dir, a.seconds, extra)
        shots.check_car(command, SCENE)
        # The viewer's `--bindings` file: no force on a wheel (on by default), the autopilot drives.
        with open(tmp / 'launcher-settings.ini', 'a') as ini:
            ini.write('\n[ffb]\nenabled = 0\n')
        if a.dry:
            print(shlex.join(command))
            return None
        began = time.time()
        # The viewer times its frames to the screen (`RBR_LATENCY`), without MangoHud's overlay (play.sh's reason).
        env.update(RBR_LATENCY='frames', MANGOHUD='0')
        done = subprocess.run(command, env=env, capture_output=True, text=True)
        output = done.stderr + done.stdout
        (run_dir / 'viewer.log').write_text(shlex.join(command) + '\n\n' + output)
    finally:
        subprocess.run(['rm', '-rf', tmp])
    logs = sorted(run_dir.glob('perf-*.log'))
    log = parse_log(logs[-1]) if logs else None
    if done.returncode or not log:
        why = f'exit {done.returncode}' if done.returncode else 'no race go in the perf log'
        sys.exit(f'{run_dir.name}: {why} after {time.time() - began:.0f} s; {run_dir / "viewer.log"}:\n{output[-1500:]}')
    if log['seconds'] < a.seconds * 0.9:
        sys.exit(f"{run_dir.name}: timed {log['seconds']:.0f} of {a.seconds} s (the window closed early?)")
    f = figures(log, output)
    f['header'] = log['header']
    return f


def scene(a):
    settings = {**SCENE['settings'], **MOST}
    settings['fullscreen'] = 'off' if a.size else 'on'
    return {**SCENE, 'settings': settings}


def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('--out', default=ROOT / 'target/bench', type=Path)
    ap.add_argument('--runs', type=int, default=RUNS, help=f'default {RUNS}')
    ap.add_argument('--seconds', type=int, default=SECONDS, help=f'timed from the go (default {SECONDS})')
    ap.add_argument('--size', help='WIDTHxHEIGHT: a window of that size instead of fullscreen')
    ap.add_argument('--build', default=ROOT / 'target/release', type=Path, help='folder with rerbr, viewer, rbr-standalone')
    ap.add_argument('--home', help="the player's files folder (default $RERBR_HOME or ~/.local/share/rerbr)")
    ap.add_argument('--args', action='append', default=[], metavar='STRING', help='extra viewer arguments, shell-split; repeatable')
    ap.add_argument('--dry', action='store_true', help='print the command instead of running it')
    ap.add_argument('--compare', type=Path, metavar='OLD_DIR', help='print the result in --out against an earlier one and exit')
    a = ap.parse_args()
    if a.compare:
        new, old = (json.loads((d / 'result.json').read_text()) for d in (a.out, a.compare))
        return compare(new, old)
    if not a.dry and not (os.environ.get('WAYLAND_DISPLAY') or os.environ.get('DISPLAY') or sys.platform != 'linux'):
        sys.exit('no display: the benchmark draws in a window (WAYLAND_DISPLAY or DISPLAY)')
    home = shots.files_folder(a.home)
    extra = [x for s in a.args for x in shlex.split(s)]
    if a.runs < 1:
        sys.exit('--runs: at least 1')
    runs = []
    for i in range(1, a.runs + 1):
        f = run_once(a, home, a.out / f'run-{i}', extra)
        if f is None:
            return
        print(row(f'run {i}', f) + (f"   drove {f['drove'][0]:.0f} m" if f['drove'] else ''), flush=True)
        runs.append(f)
    header = runs[0]['header']
    result = {'scene': scene(a), 'seconds': a.seconds, 'tools': shots.tool_versions(a.build),
              'machine': {k: header.get(k) for k in ('os', 'backend', 'adapter', 'driver', 'window', 'render size', 'present mode')},
              'runs': runs, 'median': median(runs)}
    (a.out / 'result.json').write_text(json.dumps(result, indent=1) + '\n')
    print('\n'.join(f'{k}: {v}' for k, v in result['machine'].items()))
    print_result(result)


if __name__ == '__main__':
    main()
