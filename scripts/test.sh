#!/bin/sh
# Runs the tests without taking the computer from its user: the lowest priority, half the
# processor and memory at most, and no thread in real time. Every test run goes through it.
#
#   scripts/test.sh                                  # the whole workspace
#   scripts/test.sh -p audiostrada-engine sidechain  # a crate, tests whose name has "sidechain"
#
# Arguments go to `cargo test`; those after `--` to the test binaries. TEST_JOBS (4) and
# TEST_THREADS (4) set how many crates build and how many tests run at once.
set -eu

cargo_args=""
test_args=""
seen=no
for arg in "$@"; do
    if [ "$seen" = no ] && [ "$arg" = "--" ]; then
        seen=yes
        continue
    fi
    quoted=$(printf "'%s'" "$(printf '%s' "$arg" | sed "s/'/'\\\\''/g")")
    if [ "$seen" = yes ]; then
        test_args="$test_args $quoted"
    else
        cargo_args="$cargo_args $quoted"
    fi
done
if [ $# -eq 0 ]; then
    cargo_args="--workspace"
fi

# Real-time threads would take the processor before anything else, the desktop included.
export AUDIOSTRADA_NO_REALTIME=1
jobs=${TEST_JOBS:-4}
threads=${TEST_THREADS:-4}
command="cargo test -j $jobs $cargo_args -- --test-threads=$threads $test_args"

case "$(uname -s)" in
Linux)
    low="nice -n 19"
    if command -v ionice >/dev/null 2>&1; then
        low="$low ionice -c 3"
    fi
    cores=$(nproc 2>/dev/null || echo 2)
    # A scope of its own: whatever the tests start stays within these limits.
    if systemd-run --user --scope --quiet true >/dev/null 2>&1; then
        eval "exec systemd-run --user --scope --quiet -p CPUQuota=$((cores * 50))% \
            -p MemoryMax=50% -p MemorySwapMax=0 $low $command"
    fi
    eval "exec $low $command"
    ;;
Darwin)
    eval "exec taskpolicy -b nice -n 19 $command"
    ;;
*)
    eval "exec nice -n 19 $command"
    ;;
esac
