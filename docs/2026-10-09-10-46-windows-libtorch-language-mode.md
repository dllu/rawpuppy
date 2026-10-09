# Windows LibTorch language mode

The CRLF checkout correction passed Windows graph preparation in CI run
`37967327730`. Five jobs passed; the Windows native-learning build then failed
inside LibTorch's public headers before inference tests could run. The earliest
diagnostics require C++20 for designated initializers and bit-field defaults;
subsequent overload errors occur under the binding's default C++17 command.

Configured Windows `CXXFLAGS=/std:c++20 /Zc:__cplusplus /permissive-`. The local
`torch-sys` build source selects C++17, but the `cc-rs` source appends environment
flags afterward, so the explicit mode supersedes it. Microsoft's documented
`/std:c++20` enables C++20 and conforming lookup; `/Zc:__cplusplus` reports the
actual mode to header feature guards. No binding, runtime version or upstream
header is modified. Added the same build setup to the native-runtime guide.

Linux CPU and macOS MPS real-model jobs passed again in this run. The retained
macOS manifest reports PyTorch 2.13.0 and zero error at every independent graph
probe. Windows compilation/execution must now be observed with the corrected
compiler mode. Workflow parsing, Python environment-script parsing and diff
checks passed; the full PROMPT.md objective remains active.
