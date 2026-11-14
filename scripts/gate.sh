#!/bin/bash
# The physics gate in one command. Builds the release binaries the checks run, then runs these side by side (niced):
# - standalone: drive2 attempt 5 through rbr-standalone (harness/rsf_compare.py, physics section), PASS when it prints
#   physics PASS and exits 0 (84084/84084);
# - with --hybrid, the hybrid check's merge gate (harness/rust_step.py, all bindings, 30 steps from each of the nine
#   tanner-corolla snapshots, snapH.snap-1295 and tanner-damage's 11820, 11827, 12000, one process per snapshot),
#   PASS when all 13 "run B vs run A" lines are equal and every run exits 0. The 1.02 snapshots decline the ports that
#   hold NGP's behaviour (notes/ngp.md), so those are covered by the standalone check alone;
# - with --typed, the typed storage's checks (notes/physics.md "Store seam"): twin-typed and twin-aids, the standalone
#   run's command again (its work files) with --twin typed, aids off and all on, PASS when the twin line reads equal
#   after each of 84084 steps and both round trips equal, with exit status 0 (aids on: 0, or 1 with no failure but the
#   recording's); twin-across, the same with --twin across (aids off: held over each step against the default, held
#   across steps); poison, a release build with rbr-hybrid/poison into target/poison and the drive with it, PASS as
#   the standalone check;
# - with --perf BASE_BIN, after the others: the step's cost of this build against BASE_BIN (scripts/perf-step.sh,
#   3 runs), no verdict.
# Prints one line per check and a final PASS/FAIL (exit 1). A check's full output is in /tmp/gate-<pid>-<name>.txt,
# kept when it fails. scripts/rust-check.sh stays a separate run (fmt, clippy, tests, vectors).
# Needs RBR_RECORDINGS, RBR_DATA, RBR_PATCH, RBR_NGP_PHYSICS, RBR_RSF (dev.env), --hybrid also RBR_SNAPS (default
# /tmp/rbr-snaps). PY is the Python, default this checkout's .venv, else the main checkout's.
# Usage: scripts/gate.sh [--hybrid] [--typed] [--perf BASE_BIN]
set -uo pipefail
cd "$(dirname "$0")/.."
source scripts/dev-env.sh
usage='usage: scripts/gate.sh [--hybrid] [--typed] [--perf BASE_BIN]'
hybrid=0
typed=0
perf=""
while [ $# -gt 0 ]; do
    case $1 in
    --hybrid) hybrid=1 ;;
    --typed) typed=1 ;;
    --perf) perf=${2:?$usage}; shift ;;
    *) echo "$usage" >&2; exit 2 ;;
    esac
    shift
done
missing=""
for v in RBR_RECORDINGS RBR_DATA RBR_PATCH RBR_NGP_PHYSICS RBR_RSF; do [ -n "${!v:-}" ] || missing+=" $v"; done
[ -z "$missing" ] || { echo "gate: FAIL (unset:$missing; docs/development.md \"Set up\")"; exit 1; }
snaps=${RBR_SNAPS:-/tmp/rbr-snaps}
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
[ "$hybrid" = 1 ] && packages+=(-p rbr-hybrid)
start build cargo build --release "${packages[@]}"
finish build built
[ "$fail" = 0 ] || { echo "gate: FAIL"; exit 1; }

out=$(mktemp -d /tmp/gate-XXXXXX)
trap 'rm -rf "$out"' EXIT

# The standalone drive and the hybrid snapshots run side by side, each snapshot its own rust_step.py. --typed keeps
# the drive's work files for the twins and runs the poison build and drive beside them.
keep=()
[ "$typed" = 1 ] && keep=(--keep-work)
start standalone "$PY" -I -B harness/rsf_compare.py "$RBR_RECORDINGS/rsf/drive2" --attempt 5 --sections physics \
    --out "$out/standalone" "${keep[@]}"
if [ "$typed" = 1 ]; then
    poison_run() {
        cargo build --release -j 8 --target-dir target/poison -p rbr-standalone --features rbr-hybrid/poison &&
            "$PY" -I -B harness/rsf_compare.py "$RBR_RECORDINGS/rsf/drive2" --attempt 5 --sections physics \
                --out "$out/poison" --standalone target/poison/release/rbr-standalone
    }
    start poison poison_run
fi
if [ "$hybrid" = 1 ]; then
    # rust_step.py SNAPSHOT RECORDING per snapshot in parallel; their reports in snapshot order, the first failing
    # status as the status.
    hybrid_run() {
        local snap n=0 status=0
        for snap in "$snaps"/vcu-cov.snap-*[0-9].bin "$snaps/snapH.snap-1295.bin" \
            "$snaps"/vcu-tanner-damage.snap-{11820,11827,12000}.bin; do
            local drive=tanner-corolla
            [[ $snap == */vcu-tanner-damage.* ]] && drive=tanner-damage
            "$PY" -I -B harness/rust_step.py "$snap" --recording "$RBR_RECORDINGS/drive-$drive.bin" --steps 30 \
                >"$out/hybrid-$n.txt" 2>&1 &
            n=$((n + 1))
        done
        for pid in $(jobs -p); do wait "$pid" || status=1; done
        for i in $(seq 0 $((n - 1))); do cat "$out/hybrid-$i.txt"; done
        return $status
    }
    start hybrid hybrid_run
fi

# rsf_compare's headline "attempt 5: <stage> | <physics> | ...": the physics column, PASS only with exit status 0.
standalone() {
    local physics
    physics=$(sed -n 's/^attempt [0-9]*: [^|]*| \([^|]*\) |.*/\1/p' "$1" | head -1)
    physics=${physics% }
    echo "${physics:-no physics line}"
    [ "$2" = 0 ] && [[ $physics == PASS* ]]
}
finish standalone standalone
if [ "$typed" = 1 ]; then
    # The drive's command from its summary.json, without the trace (output only, 1.4 GB), then with --twin typed; the
    # aids flags given again override the recording's.
    mapfile -d '' drive < <("$PY" -I -B -c 'import json, sys
c = json.load(open(sys.argv[1]))["physics"]["command"]
for flag in ("--trace-out", "--ffb-settings"):
    del c[c.index(flag):c.index(flag) + 2]
sys.stdout.write("\0".join(c))' "$out/standalone/5/summary.json")
    start twin-typed "${drive[@]}" --twin typed
    start twin-aids "${drive[@]}" --auto-gears on --gear-guard on --neutral-lock on --clutch-help on --twin typed
    start twin-across "${drive[@]}" --twin across
fi
if [ "$hybrid" = 1 ]; then
    # Every "run B vs run A" line equal, one per snapshot (13), and exit status 0.
    hybrid_verdict() {
        local lines equal
        lines=$(grep -c 'run B vs run A' "$1")
        equal=$(grep -c 'run B vs run A: equal apart from the excluded bytes' "$1")
        echo "$equal of $lines snapshots equal"
        [ "$2" = 0 ] && [ "$lines" = 13 ] && [ "$equal" = 13 ]
    }
    finish hybrid hybrid_verdict
fi

if [ "$typed" = 1 ]; then
    finish poison standalone
    # The twin's report line "twin KIND: <steps>; lower(lift) first side <...>, second side <...>; ..." (twin.rs),
    # KIND Typed unless given.
    twin_line() {
        local line held="84084 steps, car, wheels, solids (*) and owned bytes equal after each; lower(lift) first side "
        held+="* owned bytes equal, second side * owned bytes equal;*"
        line=$(sed -n "s/^  twin ${3:-Typed}: //p" "$1" | head -1)
        echo "${line:-no twin line}" | cut -d';' -f1-2
        [[ $line == $held ]]
    }
    twin_typed() { twin_line "$1" && [ "$2" = 0 ]; }
    twin_across() { twin_line "$1" "$2" Across && [ "$2" = 0 ]; }
    # Aids on, the run differs from the recording: exit 1 passes when none of the report's other failures shows (no
    # stop, the time card and the application state equal; headless/report.rs).
    twin_aids() {
        twin_line "$1" && [ "$2" -le 1 ] &&
            ! grep -qE "^  (stopped: |Game's time card differs|Game's application state .* differs)" "$1"
    }
    finish twin-typed twin_typed
    finish twin-aids twin_aids
    finish twin-across twin_across
fi

if [ -n "$perf" ]; then
    # After the others, so the counts run alone. perf-step.sh's last line, "B vs A: instructions ..."; only a run
    # that fails is a FAIL.
    perf_verdict() { tail -1 "$1"; [ "$2" = 0 ]; }
    start perf env PY="$PY" scripts/perf-step.sh "$perf" target/release/rbr-standalone
    finish perf perf_verdict
fi

[ "$fail" = 0 ] && echo "gate: PASS" || { echo "gate: FAIL"; exit 1; }
