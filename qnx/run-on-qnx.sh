#!/bin/sh
# Run compiled C tests on the QNX board. Copy the whole tests/build_qnx
# directory over and invoke this script from inside it:
#
#   cd /tmp/build_qnx
#   ./run-on-qnx.sh
#   SKIP='sys_epoll ptrace mkfifo' ./run-on-qnx.sh
#
# Cwd stays here so fopen("stdio/fread.in") and scandir("example_dir/") work.
# Binaries live under ./bin. A non-zero exit or a missing expected stdout/stderr
# file mismatch is a failure. Tests with no expected file only need exit 0.
set -eu

cd "$(dirname "$0")"
mkdir -p logs

ok=0
fail=0
skip=0
: > REPORT

should_skip() {
    name=$1
    [ -z "${SKIP:-}" ] && return 1
    for pat in $SKIP; do
        case "$name" in
            *$pat*) return 0 ;;
        esac
    done
    return 1
}

run_one() {
    # Neutrino has no GNU timeout(1). A hung test (select, pthread) has to
    # be killed by hand; set TIMEOUT only if a timeout(1) is on PATH.
    if [ -n "${TIMEOUT:-}" ] && command -v timeout >/dev/null 2>&1; then
        timeout "$TIMEOUT" "$@"
    else
        "$@"
    fi
}

# find -print is POSIX; do not pipe to the loop (subshell would drop counts).
list=logs/binaries.list
find bin -type f >"$list"

while IFS= read -r bin; do
    name=${bin#bin/}
    if should_skip "$name"; then
        echo "SKIP $name" | tee -a REPORT
        skip=$((skip + 1))
        continue
    fi
    echo "==> $name"
    out="logs/$name.stdout"
    err="logs/$name.stderr"
    mkdir -p "$(dirname "$out")" "$(dirname "$err")"
    rc=0
    run_one "./$bin" >"$out" 2>"$err" || rc=$?

    reason=
    if [ "$rc" -ne 0 ]; then
        reason="exit $rc"
    fi
    exp_out="expected/$name.stdout"
    exp_err="expected/$name.stderr"
    if [ -z "$reason" ] && [ -f "$exp_out" ]; then
        if ! cmp -s "$out" "$exp_out"; then
            reason="stdout mismatch"
        fi
    fi
    if [ -z "$reason" ] && [ -f "$exp_err" ]; then
        if ! cmp -s "$err" "$exp_err"; then
            reason="stderr mismatch"
        fi
    fi
    if [ -n "$reason" ]; then
        echo "FAIL $name ($reason)" | tee -a REPORT
        fail=$((fail + 1))
    else
        echo "ok   $name" | tee -a REPORT
        ok=$((ok + 1))
    fi
done <"$list"

echo
echo "summary: $ok ok, $fail failed, $skip skipped" | tee -a REPORT
[ "$fail" -eq 0 ]
