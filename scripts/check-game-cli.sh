#!/bin/sh
# Scaffold a game with `mrt-game new`, check it, and run it.
#
# The starter game is a 170-line MRT program shipped inside a Rust binary, so
# nothing else in this repository parses it: a builtin renamed or a signature
# changed would leave `mrt-game new` producing a game that does not run, and
# every other test would stay green. This is the check that notices.
#
# Needs Xvfb for the run step; without it the scaffold and the check still run
# and the run is skipped, because a build machine with no display is a normal
# place to be.
set -e
cd "$(dirname "$0")/.."

root=$(mktemp -d)
trap 'rm -rf "$root"' EXIT

bin="compiler/target/release/mrt-game"
if [ ! -x "$bin" ]; then
    echo "building mrt-game..."
    cargo build --release --manifest-path compiler/Cargo.toml --bin mrt-game
fi
bin="$(pwd)/$bin"

echo "== new =="
(cd "$root" && "$bin" new demo)
test -f "$root/demo/main.mrt" || { echo "FAIL: no main.mrt"; exit 1; }
test -f "$root/demo/README.md" || { echo "FAIL: no README.md"; exit 1; }

echo "== check =="
"$bin" check "$root/demo"

echo "== new refuses to overwrite =="
if (cd "$root" && "$bin" new demo) 2>/dev/null; then
    echo "FAIL: new overwrote an existing directory"
    exit 1
fi

echo "== a directory that is not a game =="
if "$bin" check "$root" 2>/dev/null; then
    echo "FAIL: check accepted a directory with no main.mrt"
    exit 1
fi

echo "== run =="
if ! command -v xvfb-run >/dev/null 2>&1; then
    echo "xvfb-run not found; skipping the run step."
    echo "game CLI check passed (run step skipped)."
    exit 0
fi

# The game loops until its window closes, so it is killed on purpose and the
# timeout's own exit code means success. What is actually being checked is
# that it printed nothing to stderr: a runtime error would be there.
#
# The redirect goes *inside* the inner shell on purpose. xvfb-run runs its
# command as `"$@" 2>&1`, merging the child's stderr into stdout, so a
# redirect on xvfb-run itself catches an empty stream and this check passes
# whatever the game did -- which is exactly what it did until a deliberately
# broken starter game sailed through it.
errors="$root/run.err"
xvfb-run -a --server-args="-screen 0 800x600x24" \
    sh -c "timeout 4 '$bin' run '$root/demo' 2>'$errors'" >/dev/null || true

# ALSA's C library writes to the terminal itself when there is no sound card,
# before anything in Rust can return an error. Those lines are libasound's,
# not the game's, so they are filtered out rather than counted as failure --
# and everything else still is.
real=$(grep -v '^ALSA lib' "$errors" || true)
if [ -n "$real" ]; then
    echo "FAIL: the game wrote to stderr:"
    echo "$real"
    exit 1
fi

echo "game CLI check passed."
