#!/usr/bin/env python3
"""Verify the distribution that a release would publish.

    python3 scripts/publish.py            # build, check, install, smoke-test
    python3 scripts/publish.py --keep     # leave dist/ in place afterwards

This does **not** upload. Publishing happens in `.github/workflows/release.yml`
when a `v*` tag is pushed, using PyPI Trusted Publishing -- so no API token is
stored anywhere, and the artifact that reaches PyPI is one CI built from a
tagged commit rather than one built on somebody's laptop.

What this script is for is answering "would that release be any good?" before
the tag exists, because a bad release cannot be replaced: PyPI refuses to
accept the same version twice, so a mistake costs a version number.

It checks four things, in the order they tend to break:

1. the sdist and wheel build at all;
2. `twine check` passes, so the long description renders on the project page
   rather than showing a raw-text fallback;
3. the wheel installs into a *clean* virtualenv -- which is where a missing
   package, a bad entry point or an over-eager `packages` list shows up, and
   an editable install in the working tree never will;
4. the installed `mrt` command runs an actual program, and the import package
   has the name it is supposed to have.

That last check exists because the package shipped `src` as its top-level
import name for several versions: correct-looking locally, a name collision
with every other project that made the same mistake once published.
"""

import argparse
import pathlib
import shutil
import subprocess
import sys
import tempfile

REPO = pathlib.Path(__file__).resolve().parent.parent
DIST = REPO / "dist"
# The name the wheel must install. Checked, not assumed.
IMPORT_NAME = "mrt"


def have(module: str) -> bool:
    """Is `module` importable by the interpreter that will run the build?"""
    return subprocess.run([sys.executable, "-c", f"import {module}"],
                          capture_output=True).returncode == 0


def run(*command: str, cwd: pathlib.Path = REPO) -> None:
    print(f"$ {' '.join(str(c) for c in command)}")
    result = subprocess.run(command, cwd=cwd)
    if result.returncode != 0:
        sys.exit(f"failed: {' '.join(str(c) for c in command)}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--keep", action="store_true",
                        help="leave dist/ in place instead of removing it")
    args = parser.parse_args()

    # Checked before anything is built, so a missing tool is one clear line
    # rather than a traceback halfway through a release check.
    missing = [name for name in ("build", "twine") if not have(name)]
    if missing:
        sys.exit(f"missing build tooling: {', '.join(missing)}\n"
                 f"install it with: {sys.executable} -m pip install -r requirements-dev.txt")

    print("== cleaning previous builds ==")
    for path in [DIST, REPO / "build", *REPO.glob("*.egg-info")]:
        shutil.rmtree(path, ignore_errors=True)

    print("\n== building ==")
    run(sys.executable, "-m", "build")

    print("\n== checking metadata and long description ==")
    run(sys.executable, "-m", "twine", "check", *[str(p) for p in DIST.iterdir()])

    wheels = sorted(DIST.glob("*.whl"))
    if len(wheels) != 1:
        sys.exit(f"expected exactly one wheel in {DIST}, found {len(wheels)}")

    with tempfile.TemporaryDirectory(prefix="mrt-release-") as tmp:
        venv = pathlib.Path(tmp) / "venv"
        print("\n== installing the wheel into a clean virtualenv ==")
        run(sys.executable, "-m", "venv", str(venv))
        run(str(venv / "bin" / "pip"), "install", "--quiet", str(wheels[0]))

        program = pathlib.Path(tmp) / "smoke.mrt"
        program.write_text('func main() { print("ok"); }\n')

        print("\n== running a program through the installed CLI ==")
        result = subprocess.run([str(venv / "bin" / "mrt"), str(program)],
                                capture_output=True, text=True)
        if result.returncode != 0 or result.stdout.strip() != "ok":
            sys.exit(f"installed CLI misbehaved: {result.returncode} "
                     f"{result.stdout!r} {result.stderr!r}")

        # Every import check below runs from `tmp`, never from the repository.
        # `python -c` puts the working directory on sys.path, so running these
        # from the repo root imports the source tree and reports that it
        # "works" no matter what the wheel actually contains -- and, because
        # any directory is an importable namespace package, `import src` would
        # find the Playground's React folder and report a collision that isn't
        # there. Both happened while writing this.
        print("\n== checking the installed import name ==")
        check = (
            f"import {IMPORT_NAME}, os, sys;"
            f"print(os.path.dirname({IMPORT_NAME}.__file__));"
            "sys.exit(0)"
        )
        run(str(venv / "bin" / "python"), "-c", check, cwd=pathlib.Path(tmp))

        # A top-level `src` would collide with anything else that ships one.
        stray = subprocess.run([str(venv / "bin" / "python"), "-c", "import src"],
                               cwd=tmp, capture_output=True, text=True)
        if stray.returncode == 0:
            sys.exit("the wheel installs a top-level 'src' package; fix "
                     "[tool.setuptools] packages in pyproject.toml")
        print("no stray top-level 'src' package")

    if not args.keep:
        shutil.rmtree(DIST, ignore_errors=True)

    print("\nThe distribution looks publishable. To release it:\n"
          "  1. bump `version` in pyproject.toml and merge\n"
          "  2. git tag vX.Y.Z && git push origin vX.Y.Z\n"
          "The Release workflow builds, re-checks and uploads it.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
