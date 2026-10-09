"""Prepare pinned Moebius TorchScript graphs; Rawpuppy performs inference in Rust.

Requires the upstream Moebius checkout and its inference dependencies. This tool
does not modify the checkout, photographs, or an existing Python environment.
"""
import argparse
import hashlib
import importlib.metadata
import json
import math
import subprocess
import sys
import types
from pathlib import Path

import torch
import yaml
from diffusers import AutoencoderKL

UPSTREAM_REVISION = "b88d462bacb9af6e7128a3b4cc4a07418bedfd61"


def digest(path):
    hasher = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def inference_imports(repo):
    # The upstream package initializer imports an unused teacher needing FLA.
    # Load only the student and its modules without editing upstream source.
    package = types.ModuleType("model_lib")
    package.__path__ = [str(repo / "model_lib")]
    sys.modules["model_lib"] = package
    sys.path.insert(0, str(repo))
    from model_lib.nets.unet_lambda_prune_lite import (
        UNet2DLambdaDWConvMixFFNConditionModel_prune_down_mid_up_block_8x8,
    )
    package.UNet2DLambdaDWConvMixFFNConditionModel_prune_down_mid_up_block_8x8 = (
        UNet2DLambdaDWConvMixFFNConditionModel_prune_down_mid_up_block_8x8
    )
    # Its removal initializer imports the full Python image pipeline (OpenCV,
    # teacher helpers), which the native graph preparation does not execute.
    for name, directory in [("removal", repo / "removal"), ("removal.v1_2", repo / "removal/v1_2")]:
        namespace = types.ModuleType(name)
        namespace.__path__ = [str(directory)]
        sys.modules[name] = namespace
    from removal.v1_2.removal_model import build_removal_model
    return build_removal_model


class Encoder(torch.nn.Module):
    def __init__(self, vae):
        super().__init__()
        self.vae = vae

    def forward(self, image):
        distribution = self.vae.encode(image).latent_dist
        return torch.cat([distribution.mean, distribution.std], dim=1)


class Decoder(torch.nn.Module):
    def __init__(self, vae):
        super().__init__()
        self.vae = vae

    def forward(self, latent):
        return self.vae.decode(latent).sample


class Denoiser(torch.nn.Module):
    def __init__(self, model):
        super().__init__()
        self.model = model

    def forward(self, latent, timestep, embedding_ids):
        return self.model(latent, timestep, embedding_ids).sample


def dynamic_devices(module):
    # Tensor.device is concrete during tracing. Replace device literals with the
    # first input's device so the prepared graphs can run on CPU, CUDA or MPS.
    graph = module.graph
    tensor = list(graph.inputs())[1]
    device = graph.create("prim::device", [tensor], 1)
    device.output().setType(torch._C.DeviceObjType.get())
    device.insertBefore(next(graph.nodes()))

    def visit(block):
        for node in list(block.nodes()):
            if node.kind() == "prim::Constant" and str(node.output().type()) == "Device":
                node.output().replaceAllUsesWith(device.output())
                node.destroy()
            else:
                for child in node.blocks():
                    visit(child)
    visit(graph)
    torch._C._jit_pass_dce(graph)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", required=True, type=Path)
    parser.add_argument("--models", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--device", default="cpu")
    parser.add_argument("--threads", type=int, default=8)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Choose a fresh graph output directory")
    if args.threads <= 0:
        parser.error("Threads must be positive")
    revision = subprocess.check_output(["git", "-C", str(args.repo), "rev-parse", "HEAD"], text=True).strip()
    changed = subprocess.check_output(["git", "-C", str(args.repo), "diff", "--name-only", "HEAD"], text=True).strip()
    if revision != UPSTREAM_REVISION or changed:
        parser.error("Use the unchanged pinned Moebius checkout at " + UPSTREAM_REVISION)
    torch.set_num_threads(args.threads)
    torch.manual_seed(0)
    hashes = {
        "ft_places2/diffusion_pytorch_model.bin": "6525afb888e55f9b5c74fa0a5d19ca0762d720d6c716fb0f8422fbeb6868a09a",
        "vae/diffusion_pytorch_model.bin": "a59d7ea697f2942d22002dc3469e8c53db807a6b78f7f5ec03bd4c1f70f98efe",
        "vae/config.json": "caeb94d8607dbd24acd13719c6823e2525509de8a21eb6b56cf93d5267c04694",
    }
    for name, expected in hashes.items():
        if digest(args.models / name) != expected:
            raise ValueError(f"Checkpoint hash mismatch: {name}")
    build = inference_imports(args.repo)
    config = yaml.safe_load((args.repo / "config/model_cfg/moebius.yaml").read_text())
    model = build(config, 20)
    state = torch.load(args.models / "ft_places2/diffusion_pytorch_model.bin", map_location="cpu", weights_only=True)
    model.load_state_dict(state, strict=True)
    del state
    model = model.eval().to(args.device)
    vae = AutoencoderKL.from_pretrained(args.models / "vae", local_files_only=True).eval().to(args.device)
    args.output.mkdir(parents=True, exist_ok=True)
    image = torch.randn(1, 3, 512, 512, device=args.device)
    latent = torch.randn(1, 4, 64, 64, device=args.device)
    sample = torch.randn(2, 9, 64, 64, device=args.device)
    timestep = torch.tensor([950], dtype=torch.int64, device=args.device)
    ids = torch.tensor([list(range(10, 20)), list(range(10))], dtype=torch.int64, device=args.device)
    manifest = {"version": 1, "size": 512, "scaling_factor": float(vae.config.scaling_factor),
                "source_hashes": hashes, "source_revision": revision,
                "torch_version": torch.__version__, "cpu_threads": args.threads,
                "preparation_versions": {name: importlib.metadata.version(name) for name in
                                         ("torch", "torchvision", "diffusers", "transformers", "accelerate", "timm", "einops", "pyyaml")},
                "modules": {}}
    with torch.inference_mode():
        for name, module, inputs in [
            ("encoder", Encoder(vae), (image,)),
            ("decoder", Decoder(vae), (latent,)),
            ("denoiser", Denoiser(model), (sample, timestep, ids)),
        ]:
            print(f"Preparing {name}", flush=True)
            traced = torch.jit.trace(module.eval(), inputs, check_trace=False)
            frozen = torch.jit.freeze(traced, optimize_numerics=False)
            dynamic_devices(frozen)
            expected = module(*inputs)
            trace_error = (expected-traced(*inputs)).abs().max().item()
            actual = frozen(*inputs)
            error = (expected - actual).abs().max().item()
            print(f"Numerical checks: trace={trace_error}, freeze={error}",flush=True)
            if not all(math.isfinite(value) and value <= 0.0001 for value in (trace_error, error)):
                raise ValueError(f"{name} export discrepancy: {error}")
            path = args.output / f"{name}.pt"
            temporary = path.with_suffix(".pt.part")
            frozen.save(str(temporary))
            temporary.replace(path)
            manifest["modules"][name] = {"file": path.name, "sha256": digest(path), "max_abs_error": error,
                                        "max_trace_error": trace_error}
            print(f"Verified {name}: max absolute error {error}", flush=True)
    path = args.output / "manifest.json"
    temporary = path.with_suffix(".json.part")
    temporary.write_text(json.dumps(manifest, indent=2) + "\n")
    temporary.replace(path)


if __name__ == "__main__":
    main()
