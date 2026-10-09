#!/usr/bin/env python3
"""Prepare an independent inference graph for CC BY 4.0 RawNIND Bayer weights.

The published parameter layout describes a four-level convolutional U-Net and a
pixel-shuffle head. This graph uses generic PyTorch operators; the author's GPL
implementation is an external numerical oracle, and is not copied or bundled.
The original weights are dual-licensed GPLv3 / CC BY 4.0 by their authors.
"""

import argparse
import hashlib
import importlib.util
import json
import subprocess
import sys
import types
from pathlib import Path

import torch
from torch import nn
from torch.nn import functional as F


WEIGHT_SHA256 = "c1f7de3de24cb12f4803ee65e11cfb467e9a5b80c3cb7126746ef026106c15e0"
ORACLE_REVISION = "e80fb7b14dc440bba38c01f312fd3cc02deb566c"


def digest(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


class BayerUNet(nn.Module):
    """Fixed parameter-data graph, expressed independently of the oracle code."""

    def __init__(self, state, pool, skip_first):
        super().__init__()
        self.pool = pool
        self.skip_first = skip_first
        for name, tensor in state.items():
            self.register_buffer(name.replace(".", "_"), tensor)

    def value(self, name):
        return getattr(self, name.replace(".", "_"))

    def block(self, x, name):
        for index in [0, 2]:
            x = self.convolution(x, f"{name}.{index}", 1, 1, False)
            x = F.leaky_relu(x, negative_slope=0.2)
        return x

    def convolution(self, x, name, stride, padding, transposed):
        # Encode precision per operator. F.conv2d tracing captures cuDNN's
        # default TF32 allowance, which made overlapping GPU contexts diverge.
        # This avoids changing a process-wide policy used by other models.
        return torch.ops.aten._convolution.default(
            x, self.value(name + ".weight"), self.value(name + ".bias"),
            [stride, stride], [padding, padding], [1, 1], transposed, [0, 0],
            1, False, True, True, False,
        )

    def forward(self, packed):
        features = []
        x = packed
        for level in range(1, 5):
            x = self.block(x, f"convs{level}")
            features.append(x)
            x = F.max_pool2d(x, 2) if self.pool == "max" else F.avg_pool2d(x, 2)
        x = self.block(x, "bottom")
        for level, context in enumerate(reversed(features), start=1):
            x = self.convolution(x, f"up{level}", 2, 0, True)
            x = torch.cat([context, x] if self.skip_first else [x, context], dim=1)
            x = self.block(x, f"tconvs{level}")
        x = self.convolution(x, "output_module.0", 1, 0, False)
        return F.pixel_shuffle(x, 2)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--weights", type=Path, required=True)
    parser.add_argument("--oracle-repo", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Choose a fresh output directory")
    if digest(args.weights) != WEIGHT_SHA256:
        parser.error("Weights differ from the evaluated published checkpoint")
    revision = subprocess.check_output(
        ["git", "-C", str(args.oracle_repo), "rev-parse", "HEAD"], text=True,
    ).strip()
    if revision != ORACLE_REVISION:
        parser.error("Oracle checkout differs from the inspected revision")
    torch.set_num_threads(8)
    torch.set_num_interop_threads(1)
    state = torch.load(args.weights, map_location="cpu", weights_only=True)
    if not all(t.dtype == torch.float32 and torch.isfinite(t).all() for t in state.values()):
        parser.error("Expected finite float32 parameter data")
    if tuple(state["convs1.0.weight"].shape) != (32, 4, 3, 3):
        parser.error("Unexpected input layer")
    if tuple(state["output_module.0.weight"].shape) != (12, 32, 1, 1):
        parser.error("Unexpected Bayer output head")
    # UtNet2(preupsample=False) never calls this optional RAW processing helper.
    # Stub only the unused import, so loading the oracle does not install its
    # separate image-development stack. Any unexpected call fails immediately.
    rawproc = types.ModuleType("rawnind.libs.rawproc")
    libs = types.ModuleType("rawnind.libs")
    libs.rawproc = rawproc
    package = types.ModuleType("rawnind")
    package.libs = libs
    sys.modules.update({"rawnind": package, "rawnind.libs": libs,
                        "rawnind.libs.rawproc": rawproc})
    oracle_file = args.oracle_repo / "src/rawnind/models/raw_denoiser.py"
    canonical = subprocess.check_output([
        "git", "-C", str(args.oracle_repo), "show",
        ORACLE_REVISION + ":src/rawnind/models/raw_denoiser.py",
    ])
    if hashlib.sha256(canonical).hexdigest() != digest(oracle_file):
        parser.error("Oracle source file was modified")
    spec = importlib.util.spec_from_file_location("rawpuppy_external_rawnind_oracle", oracle_file)
    external = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(external)
    oracle = external.UtNet2(in_channels=4, funit=32, preupsample=False).eval()
    oracle.load_state_dict(state, strict=True)
    torch.manual_seed(4217)
    probe = torch.rand(1, 4, 32, 48)
    candidates = []
    with torch.inference_mode():
        reference = oracle(probe)
        for pool in ["max", "average"]:
            for skip_first in [False, True]:
                model = BayerUNet(state, pool, skip_first).eval()
                error = (model(probe) - reference).abs().max().item()
                candidates.append((error, pool, skip_first))
        error, pool, skip_first = min(candidates)
        if error > 1e-5:
            raise ValueError(f"Independent graph does not match oracle: {candidates}")
        model = BayerUNet(state, pool, skip_first).eval()
        module = torch.jit.trace(model, probe, check_trace=False)
        module = torch.jit.freeze(module, optimize_numerics=False)
        checks = []
        for height, width, low, high in [(32, 48, 0., 1.), (64, 64, -0.02, 1.2),
                                        (96, 80, 0., 4.), (128, 128, 0., 0.02)]:
            sample = low + (high - low) * torch.rand(1, 4, height, width)
            expected = oracle(sample)
            got = module(sample)
            error = (got - expected).abs().max().item()
            if error > 1e-4 or got.shape != (1, 3, height * 2, width * 2):
                raise ValueError(f"Graph parity failed: {height}×{width}: {error}")
            checks.append({"packed_size": [width, height], "input_range": [low, high],
                           "max_absolute_error": error})
        args.output.mkdir(parents=True)
        graph = args.output / "bayer.pt"
        module.save(str(graph))
    manifest = {
        "version": 2,
        "architecture": "independent four-level U-Net parameter graph",
        "pool": pool,
        "skip_first": skip_first,
        "negative_slope": 0.2,
        "allow_tf32": False,
        "deterministic_convolutions": True,
        "weights_sha256": WEIGHT_SHA256,
        "weight_source": "https://drive.google.com/uc?id=1dFTLeljWi9wwojcZUsam8bE31JdYy3oM",
        "weight_authors": "Benoit Brummer and Christophe De Vleeschouwer",
        "weight_license": "CC BY 4.0, chosen from authors' GPLv3 / CC BY 4.0 dual license",
        "license_declaration": "https://github.com/trougnouf/rawnind_jddc/blob/" + ORACLE_REVISION + "/README.md",
        "oracle_revision": ORACLE_REVISION,
        "oracle_source_sha256": digest(oracle_file),
        "oracle_code_bundled": False,
        "torch_export_version": torch.__version__,
        "graph_sha256": digest(graph),
        "input": "NCHW float32 packed RGGB [R,G top-right,G bottom-left,B], before white balance",
        "output": "NCHW float32 camera-native RGB, twice spatial size, arbitrary learned gain",
        "packed_dimension_multiple": 16,
        "clipping": "none in this graph; signed and above-white probes are parity checks, not HDR quality validation",
        "checks": checks,
    }
    (args.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    (args.output / "NOTICE.txt").write_text(
        "RawNIND weights by Benoit Brummer and Christophe De Vleeschouwer.\n"
        "Weights licensed under Creative Commons Attribution 4.0 International.\n"
        "https://creativecommons.org/licenses/by/4.0/\n"
        "https://github.com/trougnouf/rawnind_jddc\n"
        "Rawpuppy converts the published parameter data to an independently expressed inference graph.\n"
        "The authors' GPL implementation is used only as an external numerical oracle.\n"
    )
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
