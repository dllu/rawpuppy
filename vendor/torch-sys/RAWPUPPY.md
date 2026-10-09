# Local torch-sys patch

This is the published `torch-sys` 0.26.0 crate from tch-rs commit
`4227b89b72059b0651ff83a38637693574e0a2d6`, with its MIT/Apache-2.0 licenses.
`UPSTREAM.json` retains the original packaged-file SHA-256 values. LibTorch
itself and model weights are not vendored.

The Windows MSVC bridge uses C++20, `/Zc:__cplusplus` and conforming lookup because
matching LibTorch 2.13 headers contain C++20 designated initializers/bit-field
defaults. The original `module` alias appears at the start of function
declarations, where C++20 parses it as a module declaration. The alias is renamed
to `torch_module` in the C/C++ bridge; exported C function names, pointer types
and the Rust API remain unchanged.

The invalid MSBuild `/p:DefineConstants=GLOG_USE_GLOG_EXPORT` compiler argument
is replaced with `cc::Build::define`. Only `build.rs`, `libtch/torch_api.h` and
`libtch/torch_api.cpp` differ from the published implementation. The local path
patch leaves Cargo's registry and other projects untouched.
