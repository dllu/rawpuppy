# Owned-process CUDA allocation probe

Use matching CUDA LibTorch headers/runtime and its C++ ABI. The helper is
diagnostic-only and affects the invoking process's caching allocator.

```sh
# Set LIBTORCH and runtime library paths as in docs/moebius.md.
clang++ -std=c++17 -O2 -shared -fPIC -D_GLIBCXX_USE_CXX11_ABI=1 \
  -I"$LIBTORCH/include" -I"$LIBTORCH/include/torch/csrc/api/include" \
  -I/usr/local/cuda/include tools/cuda_allocator_probe.cpp \
  -L"$LIBTORCH/lib" -L/usr/local/cuda/lib64 \
  -lc10_cuda -lc10 -ltorch_cuda -lcudart -o /tmp/allocator-probe.so
cargo build --release --features raw-ml,moebius --example cuda_inference_memory_probe
target/release/examples/cuda_inference_memory_probe /tmp/allocator-probe.so \
  /path/to/rawnind-graph /path/to/moebius-graphs
```

The baseline models load before the 64 MiB limit. The probe checks real allocation
errors, restores the previous fraction, then checks identical inference. Model
weights already allocated remain live; the limit rejects subsequent requests.
It creates no photographs and does not apply device-wide memory pressure. It
does not establish recovery after device loss or every possible failing operator.
