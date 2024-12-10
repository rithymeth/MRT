#!/usr/bin/env python3
import os
import sys
import subprocess

def run_command(command):
    process = subprocess.run(command, shell=True)
    if process.returncode != 0:
        print(f"Error running command: {command}")
        sys.exit(1)

def main():
    # Clean previous builds
    print("Cleaning previous builds...")
    run_command("rm -rf build/ dist/ *.egg-info")

    # Build the package
    print("Building package...")
    run_command("python -m build")

    # Upload to PyPI
    print("Uploading to PyPI...")
    run_command("python -m twine upload dist/*")

if __name__ == "__main__":
    main()
