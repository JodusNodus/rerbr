#!/usr/bin/env python3
"""Drive converted stages headless with ReRBR's autopilot and print how far it got and how far off the driveline.

    scripts/btb-autopilot.py NAME... [--tracks DIR] [--rerbr DIR] [--car N] [--top-speed M/S] [--steps N]
        [--wetness dry|damp|wet] [--laps N] [--out DIR]

NAME: part of the run name of ReRBR's physics sweep (`scripts/sweep-physics.py`), the stage's folder with dashes
(`Akagi-DH`). --tracks: the folder of stages to drive (`rbr-ac-tracks`' output; default the player's
`tracks`), linked into a temporary player folder so the launcher lists those stages. The car is a retail slot (default
2: the sweep's RSF car lives in a folder it removes), the controller's top speed 25 m/s; --wetness the surface
wetness (the `.mat` entry). Needs ReRBR (default this repository) with release builds of `rerbr` and `rbr-standalone`; the
player's folder is only read.
"""
import argparse, os, re, subprocess, sys, tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent.parent  # the repository
# The launcher's car arguments for the sweep's RSF car (the number of values each takes).
CAR = {'--car-pack': 1, '--car-model': 1, '--car-bank': 1, '--car-key': 1, '--car': 1}


def player_folder():
    if os.environ.get('RERBR_HOME'):
        return Path(os.environ['RERBR_HOME'])
    return Path(os.environ.get('XDG_DATA_HOME', Path.home() / '.local/share')) / 'rerbr'


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('names', nargs='+')
    ap.add_argument('--tracks', type=Path)
    ap.add_argument('--rerbr', type=Path, default=HERE)
    ap.add_argument('--car', default='2')
    ap.add_argument('--top-speed', default='25')
    ap.add_argument('--steps')
    ap.add_argument('--laps', help="laps of a circuit stage (`circuit=1` in track.ini): a number or `open`")
    ap.add_argument('--wetness', choices=['dry', 'damp', 'wet'])
    ap.add_argument('--out', type=Path)
    ap.add_argument('--require-finish', action='store_true', help='exit with an error if any stage does not finish')
    args = ap.parse_args()
    out = args.out or Path(tempfile.mkdtemp(prefix='autopilot-'))
    home = out / 'home'
    home.mkdir(parents=True, exist_ok=True)
    player = player_folder()
    for entry in player.iterdir():
        if entry.name != 'tracks' and not (home / entry.name).exists():
            (home / entry.name).symlink_to(entry)
    link = home / 'tracks'
    link.unlink(missing_ok=True)
    link.symlink_to((args.tracks or player / 'tracks').resolve())

    sweep = [sys.executable, '-I', '-B', str(args.rerbr / 'scripts/sweep-physics.py'), '--home', str(home), '--out',
             str(out / 'sweep'), '--dry', '--only', *args.names]
    done = subprocess.run(sweep, capture_output=True, text=True)
    if done.returncode:
        sys.exit(f'sweep-physics: {done.stderr.strip()}')
    runs = sorted((out / 'sweep/runs').glob('*/command.txt'))
    if not runs:
        sys.exit(f'no run matches {args.names} (run names: the stage folder with dashes)')
    failed = False
    for command in runs:
        argv, cmd, i = command.read_text().splitlines(), [], 0
        while i < len(argv):
            if argv[i] in CAR:
                i += 1 + CAR[argv[i]]
                continue
            if argv[i] == '--steps' and args.steps:
                argv[i + 1] = args.steps
            cmd.append(argv[i])
            i += 1
        cmd += ['--car', args.car, '--top-speed', args.top_speed]
        if args.laps:
            cmd += ['--laps', args.laps]
        if args.wetness:
            cmd += ['--wetness', args.wetness]
        done = subprocess.run(cmd, capture_output=True, text=True)
        (command.parent / 'stderr.log').write_text(done.stderr)
        progress = [l for l in done.stderr.splitlines() if l.startswith('autopilot: step')]
        off = [float(m[1]) for l in progress if (m := re.search(r'([-\d.]+) m off the driveline', l))]
        result = command.parent / 'result.txt'
        print(f'{command.parent.name}: exit {done.returncode}, max {max(off, default=float("nan")):.1f} m off the '
              f'driveline')
        print('  last:', progress[-1] if progress else done.stderr.strip()[-800:])
        if result.exists():
            print('  ' + result.read_text().strip().replace('\n', '\n  '))
        fields = dict(line.split('=', 1) for line in result.read_text().splitlines() if '=' in line) if result.exists() else {}
        failed |= done.returncode != 0 or (args.require_finish and fields.get('finished') != '1')
        print(f'  log: {command.parent / "stderr.log"}')
    if failed:
        sys.exit(1)


if __name__ == '__main__':
    main()
