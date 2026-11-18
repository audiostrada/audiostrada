#!/bin/sh
# Builds without taking the computer from its user: the lowest priority, half the processor
# and 40 % of the memory at most. Past that limit only the build is killed, not the desktop.
#
#   scripts/build.sh                                  # cargo build --release --bin audiostrada
#   scripts/build.sh --release -p audiostrada-ui      # any other cargo build arguments
#
# BUILD_JOBS (6) sets how many crates build at once; BUILD_MEMORY (40%) the memory limit.
set -eu

args=""
for arg in "$@"; do
    args="$args $(printf "'%s'" "$(printf '%s' "$arg" | sed "s/'/'\\\\''/g")")"
done
if [ $# -eq 0 ]; then
    args="--release --bin audiostrada"
fi

command="cargo build -j ${BUILD_JOBS:-6} $args"

case "$(uname -s)" in
Linux)
    low="nice -n 19"
    if command -v ionice >/dev/null 2>&1; then
        low="$low ionice -c 3"
    fi
    cores=$(nproc 2>/dev/null || echo 2)
    if systemd-run --user --scope --quiet true >/dev/null 2>&1; then
        eval "exec systemd-run --user --scope --quiet -p CPUQuota=$((cores * 50))% \
            -p MemoryMax=${BUILD_MEMORY:-40%} -p MemorySwapMax=0 $low $command"
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
