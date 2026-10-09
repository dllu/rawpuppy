#!/usr/bin/env python3
"""Prepare verified real Moebius graphs in a fresh CI scratch directory."""

import argparse
import shutil
import subprocess
import sys
import urllib.request
from pathlib import Path

from export_moebius import UPSTREAM_REVISION, digest


ARTIFACTS = [
    ("ft_places2/diffusion_pytorch_model.bin",
     "https://huggingface.co/hustvl/Moebius/resolve/cd01f47fb648219d3fa605806c5ce00b713faa5e/ft_places2/diffusion_pytorch_model.bin",
     "6525afb888e55f9b5c74fa0a5d19ca0762d720d6c716fb0f8422fbeb6868a09a"),
    ("vae/diffusion_pytorch_model.bin",
     "https://huggingface.co/hustvl/PixelHacker/resolve/012fd343158936a265b8a0ee38a791a7a2841f45/vae/diffusion_pytorch_model.bin",
     "a59d7ea697f2942d22002dc3469e8c53db807a6b78f7f5ec03bd4c1f70f98efe"),
    ("vae/config.json",
     "https://huggingface.co/hustvl/PixelHacker/resolve/012fd343158936a265b8a0ee38a791a7a2841f45/vae/config.json",
     "caeb94d8607dbd24acd13719c6823e2525509de8a21eb6b56cf93d5267c04694"),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--models", type=Path, help="Read existing pinned weights instead of downloading")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    models = args.models or args.output / "models"
    for name, url, expected in ARTIFACTS:
        path = models / name
        if args.models is None:
            path.parent.mkdir(parents=True, exist_ok=True)
            with urllib.request.urlopen(url, timeout=60) as response, path.open("xb") as target:
                shutil.copyfileobj(response, target, length=1024 * 1024)
        if digest(path) != expected:
            raise ValueError("Pinned Moebius checkpoint identity mismatch: " + name)
    source = args.output / "source"
    subprocess.run(["git", "init", str(source)], check=True)
    subprocess.run(["git", "-C", str(source), "config", "core.autocrlf", "false"], check=True)
    subprocess.run(["git", "-C", str(source), "config", "core.eol", "lf"], check=True)
    subprocess.run(["git", "-C", str(source), "remote", "add", "origin", "https://github.com/hustvl/Moebius.git"], check=True)
    subprocess.run(["git", "-C", str(source), "fetch", "--depth", "1", "origin", UPSTREAM_REVISION], check=True)
    subprocess.run(["git", "-C", str(source), "checkout", "--detach", "FETCH_HEAD"], check=True)
    subprocess.run([
        sys.executable, str(Path(__file__).with_name("export_moebius.py")),
        "--repo", str(source), "--models", str(models),
        "--output", str(args.output / "graph"), "--device", "cpu", "--threads", "2",
    ], check=True)


if __name__ == "__main__":
    main()
