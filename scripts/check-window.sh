#!/bin/sh
# Open a real window, present real frames, and check it worked.
#
# Not a `cargo test`: winit insists its event loop is built on the main
# thread, and cargo runs tests on spawned ones. So the check is an example
# binary, and this script is what supplies it a display.
#
# On a machine with a desktop, `cargo run --example smoke -p mrt-window` does
# the same thing in a window you can watch. This path is for the machines that
# have no desktop, which includes CI.
#
# Needs: Xvfb, and libxkbcommon-x11-0 (winit dlopens it at runtime, so a build
# succeeding says nothing about whether this will).
set -e
cd "$(dirname "$0")/.."

if ! command -v xvfb-run >/dev/null 2>&1; then
    echo "xvfb-run not found; skipping the window check."
    echo "Install it, or run 'cargo run --example smoke -p mrt-window' on a desktop."
    exit 0
fi

output=$(timeout 120 xvfb-run -a --server-args="-screen 0 640x480x24" \
    cargo run --quiet --manifest-path compiler/Cargo.toml --example smoke -p mrt-window 2>&1)
status=$?

echo "$output"
if [ $status -ne 0 ] || ! echo "$output" | grep -q '^ok$'; then
    echo "window check FAILED"
    exit 1
fi
echo "window check passed"
