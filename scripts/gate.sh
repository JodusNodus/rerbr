#!/bin/bash
# The physics gate in one command. Builds the release binaries the checks run, then runs these side by side (niced):
# - standalone: drive2 attempt 5 through rbr-standalone (harness/rsf_compare.py, physics section), PASS when it prints
#   physics PASS and exits 0 (84084/84084);
# - poison: a release build with rbr-step/poison into target/poison (a read of a typed-state word from the image
#   while a step holds it panics) and the drive with it, PASS as the standalone check;
# - aids: the drive's command again (its work files) with every driving aid on, the paths drive2 (aids off) does not
#   reach: PASS on exit status 0, or 1 with no failure but the recording's (no stop, the time card and the application
#   state equal; headless/report.rs);
# - tarmac, gravel1, gravel2, retail: the other NGP drives that run equal, PASS as the standalone check: drive1 attempt 2
#   (tarmac, Fiesta Rally3, 103802; recorded before GAM1's damage level, on off, with drive2 attempt 1's setup),
#   rsf-test1 and rsf-test2 attempt 1 (gravel, map 54, Evo IX N4, 32623 and 58619); retail: ngp-pair1 (NGP 7.4.778,
#   retail car slot 6 on map 54) to 13043, where its playback's controls end;
# - with --perf BASE_BIN, after the others: the step's cost of this build against BASE_BIN (scripts/perf-step.sh,
#   3 runs), no verdict.
# Prints one line per check and a final PASS/FAIL (exit 1). A check's full output is in /tmp/gate-<pid>-<name>.txt,
# kept when it fails. scripts/rust-check.sh stays a separate run (fmt, clippy, tests).
# Needs RBR_RECORDINGS, RBR_DATA, RBR_PATCH, RBR_NGP_PHYSICS, RBR_RSF (dev.env). PY is the Python, default this checkout's .venv, else the main checkout's.
# Usage: scripts/gate.sh [--perf BASE_BIN]
set -uo pipefail
cd "$(dirname "$0")/.."
source scripts/dev-env.sh
usage='usage: scripts/gate.sh [--perf BASE_BIN]'
perf=""
while [ $# -gt 0 ]; do
    case $1 in
    --perf) perf=${2:?$usage}; shift ;;
    *) echo "$usage" >&2; exit 2 ;;
    esac
    shift
done
missing=""
for v in RBR_RECORDINGS RBR_DATA RBR_PATCH RBR_NGP_PHYSICS RBR_RSF; do [ -n "${!v:-}" ] || missing+=" $v"; done
[ -z "$missing" ] || { echo "gate: FAIL (unset:$missing; docs/development.md \"Set up\")"; exit 1; }
if [ -z "${PY:-}" ]; then
    PY=.venv/bin/python
    [ -x "$PY" ] || PY=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")/.venv/bin/python
fi
[ -x "$PY" ] || { echo "gate: FAIL (no Python at $PY; set PY)"; exit 1; }
[ -z "$perf" ] || [ -x "$perf" ] || { echo "gate: FAIL (--perf: no binary $perf)"; exit 1; }
fail=0
renice -n 10 $$ >/dev/null # every check niced

# start NAME CMD...: runs CMD in the background into /tmp/gate-<pid>-NAME.txt; finish NAME VERDICT_CMD waits for
# it, and VERDICT_CMD (the log and the exit status) prints the line shown after "NAME:", exiting non-zero for a FAIL.
start() {
    local name=$1
    shift
    printf -v "began_${name//-/_}" %s "$SECONDS"
    "$@" >"/tmp/gate-$$-$name.txt" 2>&1 &
    printf -v "pid_${name//-/_}" %s "$!"
}
finish() {
    local name=$1 verdict=$2 log=/tmp/gate-$$-$1.txt status line pid=pid_${1//-/_} began=began_${1//-/_}
    wait "${!pid}"
    status=$?
    if line=$($verdict "$log" "$status"); then
        echo "$name: ok, $line ($((SECONDS - ${!began})) s)"
        rm -f "$log"
    else
        echo "$name: FAIL, $line ($log)"
        fail=1
    fi
}

built() { [ "$2" = 0 ] && echo "release" || { tail -1 "$1"; return 1; }; }
packages=(-p rbr-standalone -p rbr-formats)
start build cargo build --release "${packages[@]}"
finish build built
[ "$fail" = 0 ] || { echo "gate: FAIL"; exit 1; }

out=$(mktemp -d /tmp/gate-XXXXXX)
trap 'rm -rf "$out"' EXIT

# The standalone drive (its work files kept for the aids run) and the poison build and drive beside it.
start standalone "$PY" -I -B harness/rsf_compare.py "$RBR_RECORDINGS/rsf/drive2" --attempt 5 --sections physics \
    --out "$out/standalone" --keep-work
poison_run() {
    cargo build --release -j 8 --target-dir target/poison -p rbr-standalone --features rbr-step/poison &&
        "$PY" -I -B harness/rsf_compare.py "$RBR_RECORDINGS/rsf/drive2" --attempt 5 --sections physics \
            --out "$out/poison" --standalone target/poison/release/rbr-standalone
}
start poison poison_run
drive() {
    local session=$1 attempt=$2
    shift 2
    "$PY" -I -B harness/rsf_compare.py "$RBR_RECORDINGS/rsf/$session" --attempt "$attempt" --sections physics \
        --out "$out/$session-$attempt" "$@"
}
start tarmac drive drive1 2 --damage off --setup "$RBR_RECORDINGS/rsf/drive2/1/setup.lsp"
start gravel1 drive rsf-test1 1
start gravel2 drive rsf-test2 1
start retail drive ngp-pair1 1 --steps 13043

# rsf_compare's headline "attempt 5: <stage> | <physics> | ...": the physics column, PASS only with exit status 0.
standalone() {
    local physics
    physics=$(sed -n 's/^attempt [0-9]*: [^|]*| \([^|]*\) |.*/\1/p' "$1" | head -1)
    physics=${physics% }
    echo "${physics:-no physics line}"
    [ "$2" = 0 ] && [[ $physics == PASS* ]]
}
finish standalone standalone
# The drive's command from its summary.json, without the trace (output only, 1.4 GB); the aids flags given again
# override the recording's.
mapfile -d '' drive < <("$PY" -I -B -c 'import json, sys
c = json.load(open(sys.argv[1]))["physics"]["command"]
for flag in ("--trace-out", "--ffb-settings"):
    del c[c.index(flag):c.index(flag) + 2]
sys.stdout.write("\0".join(c))' "$out/standalone/5/summary.json")
start aids "${drive[@]}" --auto-gears on --gear-guard on --neutral-lock on --clutch-help on
aids() {
    grep -om1 "[0-9]* of [0-9]* steps run" "$1" || echo "no run line"
    [ "$2" -le 1 ] && ! grep -qE "^  (stopped: |Game's time card differs|Game's application state .* differs)" "$1"
}
finish poison standalone
finish aids aids
finish tarmac standalone
finish gravel1 standalone
finish gravel2 standalone
finish retail standalone

if [ -n "$perf" ]; then
    # After the others, so the counts run alone. perf-step.sh's last line, "B vs A: instructions ..."; only a run
    # that fails is a FAIL.
    perf_verdict() { tail -1 "$1"; [ "$2" = 0 ]; }
    start perf env PY="$PY" scripts/perf-step.sh "$perf" target/release/rbr-standalone
    finish perf perf_verdict
fi

[ "$fail" = 0 ] && echo "gate: PASS" || { echo "gate: FAIL"; exit 1; }
