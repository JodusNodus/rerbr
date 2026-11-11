"""pad_drive.py: drive a flat pad by hand, with the launcher's saved car, controls and settings.

  python3 -I -B harness/pad_drive.py [--material 49] [--physics ngp|experimental|experimental-brush]
      [--bin-dir target/release] [--print] [-- EXTRA viewer ARGS]

The launcher has no pad among its stages, so this takes the command its Start button would run (`rerbr --command`, the
saved selection: car, controls, graphics, options) and puts the pad of one material in the stage's place: a level 2 km
square (`rbr_btb::pad`, `--stage pad-ID`; 49 is the gravel of RSF 524 Hassi, 17 the firmest gravel, 33 the loosest,
37 dry tarmac). The stage's own options (sky, pace notes, co-driver, clock) are dropped, the replay is saved beside
the launcher's as `<time>-pad-<ID>.rrp` (`replay_gg.py` reads it), and `--physics` goes to the launcher as its session
flag. The pad is written on first use by a one-step headless run of `rbr-standalone`, whose output names its folder.
`--print` shows the command without starting it.
"""
import argparse
import os
import re
import subprocess
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
# The viewer's options that belong to the stage the launcher had selected, with the number of values each takes.
STAGE_OPTIONS = {"--sky": 1, "--pacenotes": 1, "--clock": 1, "--codriver-mod": 1, "--codriver-voice": 1,
                 "--codriver-style": 1, "--codriver-ranges": 1, "--fog": 1, "--clouds": 1, "--precipitation": 1,
                 "--ghost": 1, "--save-ghost": 1, "--best-ghost": 1, "--ghost-weather": 1}


def launcher_command(rerbr, physics):
    """The environment and the argument vector `rerbr --command` prints."""
    cmd = [rerbr, "--command"] + (["--physics", physics] if physics else [])
    out = subprocess.run(cmd, capture_output=True, text=True)
    if out.returncode != 0:
        sys.exit(f"{' '.join(cmd)}: {out.stderr.strip()}")
    env, args = {}, []
    for line in out.stdout.splitlines():
        m = re.match(r"([A-Z_]+)=(.*)", line)
        if m and not args:
            env[m.group(1)] = m.group(2)
        elif line:
            args.append(line)
    return env, args


def value(args, flag):
    return args[args.index(flag) + 1] if flag in args else None


def pad_folder(standalone, stepping, env, name):
    """The pad's prepared stage (its files' path without the extension), written by a one-step run if missing."""
    cmd = [standalone, "--autopilot", os.devnull, "--boot", "--stage", name, "--steps", "1"]
    for flag in ("--data", "--ngp-physics", "--car-pack", "--car-model", "--car-key"):
        if value(stepping, flag) is not None:
            cmd += [flag, value(stepping, flag)]
    if "--car" in stepping:
        cmd += ["--car", value(stepping, "--car")]
    out = subprocess.run(cmd, capture_output=True, text=True, env={**os.environ, **env})
    m = re.search(rf"pad {re.escape(name)} is (.+)", out.stdout + out.stderr)
    if not m:
        sys.exit(f"no pad from {' '.join(cmd)}:\n{(out.stdout + out.stderr)[-2000:]}")
    return m.group(1).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--material", type=int, default=49)
    parser.add_argument("--physics")
    parser.add_argument("--bin-dir", default=os.path.join(REPO, "target", "release"))
    parser.add_argument("--print", action="store_true")
    parser.add_argument("extra", nargs="*", help="after --: more viewer arguments")
    args = parser.parse_args()
    name = {37: "pad-tarmac", 17: "pad-gravel", 101: "pad-snow"}.get(args.material, f"pad-{args.material}")
    env, command = launcher_command(os.path.join(args.bin_dir, "rerbr"), args.physics)
    live = command.index("--live")
    viewer, stepping = command[:live], command[live:]
    stage = pad_folder(stepping[1], stepping, env, name)
    # The viewer: its program, the stage, then options.
    shown, i = [viewer[0], stage], 2
    while i < len(viewer):
        flag = viewer[i]
        if flag in STAGE_OPTIONS:
            i += 1 + STAGE_OPTIONS[flag]
            continue
        if flag == "--save-replay":
            folder = os.path.dirname(viewer[i + 1])
            shown += [flag, os.path.join(folder, f"{time.time_ns()}-{name}.rrp")]
            i += 2
            continue
        if flag == "--title":
            shown += [flag, f"{name} · {viewer[i + 1].split(' · ')[-1]}"]
            i += 2
            continue
        shown.append(flag)
        i += 1
    stepping[stepping.index("--stage") + 1] = name
    for flag in ("--start-time",):
        if flag in stepping:
            k = stepping.index(flag)
            del stepping[k:k + 2]
    final = shown + args.extra + stepping
    if args.print:
        print("\n".join([f"{k}={v}" for k, v in env.items()] + final))
        return
    os.execvpe(final[0], final, {**os.environ, **env})


if __name__ == "__main__":
    main()
