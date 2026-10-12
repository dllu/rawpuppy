#!/usr/bin/env bash
# Source this file to use a self-contained Linux PyTorch/LibTorch installation.

rawpuppy_torch_root="${RAWPUPPY_TORCH_ROOT:-${XDG_CACHE_HOME:-$HOME/.cache}/rawpuppy/evaluation/torch-2.13}"
if [ ! -f "$rawpuppy_torch_root/torch/lib/libtorch.so" ]; then
    printf 'LibTorch is missing from %s; install Torch as described in docs/moebius.md.\n' "$rawpuppy_torch_root" >&2
    unset rawpuppy_torch_root
    return 1
fi

export LIBTORCH="$rawpuppy_torch_root/torch"
export LIBTORCH_CXX11_ABI=1
export CXX="${CXX:-clang++}"
export CXXFLAGS="${CXXFLAGS:--g0}"
unset LIBTORCH_USE_PYTORCH

rawpuppy_library_path="$LIBTORCH/lib"
for rawpuppy_library_dir in "$rawpuppy_torch_root"/nvidia/*/lib; do
    if [ -d "$rawpuppy_library_dir" ]; then
        rawpuppy_library_path="$rawpuppy_library_path:$rawpuppy_library_dir"
    fi
done
# Replace the loader path so another project's Torch/CUDA libraries cannot mix in.
export LD_LIBRARY_PATH="$rawpuppy_library_path"
unset rawpuppy_torch_root rawpuppy_library_path rawpuppy_library_dir
