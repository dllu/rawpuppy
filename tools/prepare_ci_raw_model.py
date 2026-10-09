#!/usr/bin/env python3
"""Prepare the real, attributed RawNIND graph in a fresh CI scratch directory."""

import argparse
import hashlib
import subprocess
import sys
import urllib.request
from pathlib import Path

from export_rawnind import ORACLE_REVISION, WEIGHT_SHA256


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    weights = args.output / "weights.pt"
    url = "https://drive.google.com/uc?export=download&id=1dFTLeljWi9wwojcZUsam8bE31JdYy3oM"
    hasher = hashlib.sha256()
    length = 0
    with urllib.request.urlopen(url, timeout=60) as response, weights.open("xb") as target:
        while chunk := response.read(1024 * 1024):
            length += len(chunk)
            if length > 31_059_270:
                raise ValueError("Published checkpoint has unexpected size")
            hasher.update(chunk)
            target.write(chunk)
    if length != 31_059_270 or hasher.hexdigest() != WEIGHT_SHA256:
        raise ValueError("Published checkpoint identity mismatch")
    oracle = args.output / "oracle"
    subprocess.run(["git", "init", str(oracle)], check=True)
    subprocess.run(["git", "-C", str(oracle), "remote", "add", "origin",
                    "https://github.com/trougnouf/rawnind_jddc.git"], check=True)
    subprocess.run(["git", "-C", str(oracle), "fetch", "--depth", "1", "origin",
                    ORACLE_REVISION], check=True)
    subprocess.run(["git", "-C", str(oracle), "checkout", "--detach", "FETCH_HEAD"], check=True)
    subprocess.run([
        sys.executable, str(Path(__file__).with_name("export_rawnind.py")),
        "--weights", str(weights), "--oracle-repo", str(oracle),
        "--output", str(args.output / "graph"),
    ], check=True)


if __name__ == "__main__":
    main()
