#!/usr/bin/env bash
set -euo pipefail

rawpuppy_project_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
source "$rawpuppy_project_dir/tools/libtorch-env.sh"

exec cargo run --release --all-features --locked \
  --manifest-path "$rawpuppy_project_dir/Cargo.toml" -- "$@"
