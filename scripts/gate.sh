#!/bin/bash
# The physics gate in one command. Builds the release binaries the checks run, then runs these side by side (niced):
# - standalone: drive2 attempt 5 through rbr-standalone (harness/rsf_compare.py, physics section), PASS when it prints
#   physics PASS and exits 0 (84084/84084);
# - with --hybrid, the hybrid check's merge gate (harness/rust_step.py, all bindings, 30 steps from each of the nine
#   tanner-corolla snapshots, snapH.snap-1295 and tanner-damage's 11820, 11827, 12000, one process per snapshot),
#   PASS when all 13 "run B vs run A" lines are equal and every run exits 0. The 1.02 snapshots decline the ports that
#   hold NGP's behaviour (notes/ngp.md), so those are covered by the standalone check alone;
# - with --perf BASE_BIN, after the others: the step's cost of this build against BASE_BIN (scripts/perf-step.sh,
#   3 runs), no verdict.
# Prints one line per check and a final PASS/FAIL (exit 1). A check's full output is in /tmp/gate-<pid>-<name>.txt,
# kept when it fails. scripts/rust-check.sh stays a separate run (fmt, clippy, tests, vectors).
# Needs RBR_RECORDINGS, RBR_DATA, RBR_PATCH, RBR_NGP_PHYSICS, RBR_RSF (dev.env), --hybrid also RBR_SNAPS (default
# /tmp/rbr-snaps). PY is the Python, default this checkout's .venv, else the main checkout's.
# Usage: scripts/gate.sh [--hybrid] [--perf BASE_BIN]
set -uo pipefail
cd "$(dirname "$0")/.."
source scripts/dev-env.sh
usage='usage: scripts/gate.sh [--hybrid] [--perf BASE_BIN]'
hybrid=0
perf=""
while [ $# -gt 0 ]; do
    case $1 in
    --hybrid) hybrid=1 ;;
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
    printf -v "began_$name" %s "$SECONDS"
    "$@" >"/tmp/gate-$$-$name.txt" 2>&1 &
    printf -v "pid_$name" %s "$!"
}
finish() {
    local name=$1 verdict=$2 log=/tmp/gate-$$-$1.txt status line pid=pid_$1 began=began_$1
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

# The standalone drive and the hybrid snapshots run side by side, each snapshot its own rust_step.py.
start standalone "$PY" -I -B harness/rsf_compare.py "$RBR_RECORDINGS/rsf/drive2" --attempt 5 --sections physics \
    --out "$out/standalone"
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

if [ -n "$perf" ]; then
    # After the others, so the counts run alone. perf-step.sh's last line, "B vs A: instructions ..."; only a run
    # that fails is a FAIL.
    perf_verdict() { tail -1 "$1"; [ "$2" = 0 ]; }
    start perf env PY="$PY" scripts/perf-step.sh "$perf" target/release/rbr-standalone
    finish perf perf_verdict
fi

[ "$fail" = 0 ] && echo "gate: PASS" || { echo "gate: FAIL"; exit 1; }
