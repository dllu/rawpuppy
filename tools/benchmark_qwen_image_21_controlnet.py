#!/usr/bin/env python3
"""Research-only evaluation of Alibaba PAI's explicit-mask Qwen 2.1 adapter.

Uses an unchanged, pinned VideoX-Fun checkout and local verified checkpoints.
Cases are a JSON list of objects with name, image, mask and prompt fields.
"""

import argparse
import importlib
import importlib.metadata
import json
import subprocess
import sys
import time
import types
from pathlib import Path

import numpy as np
import torch
from PIL import Image, ImageFilter
from safetensors import safe_open
from safetensors.torch import load_file

from benchmark_qwen_image_21 import digest, model_identity


UPSTREAM = "4b7b6402a1e0f0406bd6801fb66c0a00bd922621"
ADAPTER_REVISION = "8a4702014d4dabb5f896fcba917e2ee0a961465f"
ADAPTER_SHA256 = "65d6b66d734da9e7ff5ef04e7db3a133553a52a3f29a7fcb3e9cce8fa21dcfcd"


def upstream_classes(root):
    """Import only the released Qwen modules, omitting unrelated video imports.

    Namespace packages replace aggregator __init__ imports in this process only.
    Every evaluated model, attention and pipeline module remains upstream code.
    """
    revision = subprocess.check_output(
        ["git", "-C", str(root), "rev-parse", "HEAD"], text=True
    ).strip()
    changes = subprocess.check_output(
        ["git", "-C", str(root), "diff", "--name-only", "HEAD"], text=True
    ).strip()
    if revision != UPSTREAM or changes:
        raise ValueError("Use an unchanged VideoX-Fun checkout at " + UPSTREAM)
    for name in ("videox_fun", "videox_fun.dist", "videox_fun.models", "videox_fun.pipeline"):
        module = types.ModuleType(name)
        module.__path__ = [str(root.joinpath(*name.split(".")))]
        sys.modules[name] = module
    dist = sys.modules["videox_fun.dist"]
    fuser = importlib.import_module("videox_fun.dist.fuser")
    for name in ("get_sequence_parallel_rank", "get_sequence_parallel_world_size", "sequence_parallel_all_gather"):
        setattr(dist, name, getattr(fuser, name))
    dist.QwenImage21MultiGPUsAttnProcessor = importlib.import_module(
        "videox_fun.dist.qwenimage21_xfuser"
    ).QwenImage21MultiGPUsAttnProcessor
    models = sys.modules["videox_fun.models"]
    for filename, names in (
        ("qwenimage21_transformer2d", ("QwenImage21KVCache", "QwenImage21Transformer2DModel")),
        ("qwenimage21_transformer2d_control", ("QwenImage21ControlTransformer2DModel",)),
        ("qwenimage21_vae", ("AutoencoderKLQwenImage21",)),
    ):
        source = importlib.import_module("videox_fun.models." + filename)
        for name in names:
            setattr(models, name, getattr(source, name))
    # The old pipeline is imported only for its output dataclass. Its type
    # imports use the corresponding public classes, with no model instantiated.
    for library, names in (
        ("transformers", ("Qwen3VLForConditionalGeneration", "Qwen3VLProcessor", "Qwen2_5_VLForConditionalGeneration", "Qwen2Tokenizer")),
        ("diffusers", ("AutoencoderKLQwenImage", "QwenImageTransformer2DModel")),
    ):
        source = importlib.import_module(library)
        for name in names:
            setattr(models, name, getattr(source, name))
    pipeline = importlib.import_module(
        "videox_fun.pipeline.pipeline_qwenimage21_control"
    ).QwenImage21ControlPipeline
    return models, pipeline


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--upstream", type=Path, required=True)
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--adapter", type=Path, required=True)
    parser.add_argument("--cases", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--steps", type=int, default=40)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--iterations", type=int, default=2)
    parser.add_argument("--mask-padding", type=int, default=0)
    args = parser.parse_args()
    if min(args.steps, args.iterations) <= 0 or args.mask_padding < 0:
        parser.error("Steps/iterations must be positive and padding nonnegative")
    if args.output.exists():
        parser.error("Choose a new output directory")
    if not torch.cuda.is_available():
        parser.error("This measurement requires CUDA")
    print("Verifying the adapter and base checkpoint identities", flush=True)
    if digest(args.adapter) != ADAPTER_SHA256:
        parser.error("Adapter checkpoint SHA-256 mismatch")
    cases = json.loads(args.cases.read_text())
    if not cases:
        parser.error("Provide at least one case")
    loaded_cases = []
    names = set()
    for case in cases:
        name = case["name"]
        if name in names or not name or Path(name).name != name or name in (".", ".."):
            parser.error("Case names must be unique directory names")
        names.add(name)
        image = Image.open(case["image"]).convert("RGB")
        mask = Image.open(case["mask"]).convert("L")
        if image.size != mask.size or any(n % 32 for n in image.size):
            parser.error("Use matching image/mask dimensions divisible by 32")
        loaded_cases.append((case, image, mask))
    torch.set_num_threads(8)
    torch.set_num_interop_threads(1)
    models, pipeline_class = upstream_classes(args.upstream)
    metadata = args.model / ".cache/huggingface/download/model_index.json.metadata"
    distribution = importlib.metadata.distribution("diffusers")
    manifest = {
        "purpose": "research/evaluation only; Qwen research license",
        "benchmark_sha256": digest(Path(__file__)),
        "upstream_revision": UPSTREAM,
        "import_scope": "unchanged Qwen modules; unrelated aggregator imports omitted",
        "base_model_revision_from_download_metadata": metadata.read_text().splitlines()[0] if metadata.exists() else None,
        "base_model_sha256": model_identity(args.model),
        "adapter_revision": ADAPTER_REVISION,
        "adapter_sha256": ADAPTER_SHA256,
        "versions": {name: importlib.metadata.version(name) for name in ("torch", "diffusers", "transformers", "accelerate")},
        "pipeline_direct_url": json.loads(distribution.read_text("direct_url.json") or "{}"),
        "device": torch.cuda.get_device_name(),
        "dtype": "bfloat16", "steps": args.steps, "seed": args.seed,
        "mask_padding": args.mask_padding, "control_context_scale": 1.0,
        "true_cfg_scale": 1.0, "use_kv_cache": True,
        "loader": "CPU load followed by to(cuda); no offload/quantization/compile",
        "cases": [],
    }
    from diffusers import FlowMatchEulerDiscreteScheduler

    started = time.perf_counter()
    transformer = models.QwenImage21ControlTransformer2DModel.from_pretrained(
        str(args.model), subfolder="transformer", low_cpu_mem_usage=True,
        torch_dtype=torch.bfloat16,
        transformer_additional_kwargs={"control_layers": list(range(0, 32, 2)), "control_in_dim": 129},
    )
    # The upstream loader can silently skip mismatched base weights. Require
    # every base key and shape to match the instantiated control transformer.
    expected = transformer.state_dict()
    base_keys = set()
    for shard in (args.model / "transformer").glob("*.safetensors"):
        with safe_open(shard, framework="pt") as handle:
            for key in handle.keys():
                if key not in expected or list(expected[key].shape) != handle.get_slice(key).get_shape():
                    raise ValueError("Base checkpoint key/shape mismatch: " + key)
                base_keys.add(key)
    if any(key not in base_keys for key in expected if not key.startswith(("control_blocks.", "control_img_in."))):
        raise ValueError("Base checkpoint is missing non-control parameters")
    state = load_file(str(args.adapter))
    control_keys = {key for key in expected if key.startswith(("control_blocks.", "control_img_in."))}
    if set(state) != control_keys:
        raise ValueError("Adapter does not exactly cover the configured control parameters")
    missing, unexpected = transformer.load_state_dict(state, strict=False)
    if unexpected or set(missing) != base_keys:
        raise ValueError("Unexpected adapter load result")
    del state, expected
    pipe = pipeline_class(
        transformer=transformer,
        vae=models.AutoencoderKLQwenImage21.from_pretrained(args.model, subfolder="vae", local_files_only=True, torch_dtype=torch.bfloat16),
        text_encoder=models.Qwen3VLForConditionalGeneration.from_pretrained(args.model, subfolder="text_encoder", local_files_only=True, dtype=torch.bfloat16),
        processor=models.Qwen3VLProcessor.from_pretrained(args.model, subfolder="processor", local_files_only=True),
        scheduler=FlowMatchEulerDiscreteScheduler.from_pretrained(args.model, subfolder="scheduler", local_files_only=True),
    ).to("cuda")
    torch.cuda.synchronize()
    manifest["load_seconds"] = time.perf_counter() - started
    args.output.mkdir()
    print(json.dumps({"load_seconds": manifest["load_seconds"]}), flush=True)
    with torch.inference_mode():
        for case, image, mask in loaded_cases:
            output = args.output / case["name"]
            output.mkdir()
            inference_mask = mask.filter(ImageFilter.MaxFilter(2 * args.mask_padding + 1)) if args.mask_padding else mask
            inference_mask.save(output / "inference-mask.png")
            record = {**case, "size": image.size, "image_sha256": digest(Path(case["image"])), "mask_sha256": digest(Path(case["mask"])), "inference_mask_sha256": digest(output / "inference-mask.png"), "runs": []}
            manifest["cases"].append(record)
            for iteration in range(args.iterations):
                torch.cuda.reset_peak_memory_stats()
                started = time.perf_counter()
                native = pipe(
                    prompt=case["prompt"], image=image, mask_image=inference_mask,
                    control_image=None, height=image.height, width=image.width,
                    generator=torch.Generator("cuda").manual_seed(args.seed),
                    true_cfg_scale=1.0, num_inference_steps=args.steps,
                    control_context_scale=1.0, use_kv_cache=True,
                ).images[0]
                torch.cuda.synchronize()
                elapsed = time.perf_counter() - started
                if native.size != image.size:
                    raise ValueError("Unexpected output dimensions")
                raw = native.convert("RGB")
                composed = Image.composite(raw, image, mask)
                outside = np.asarray(mask) == 0
                delta = np.abs(np.asarray(raw).astype(np.int16) - np.asarray(image))
                composed_delta = np.abs(np.asarray(composed).astype(np.int16) - np.asarray(image))
                alpha = np.asarray(native.getchannel("A")) if "A" in native.getbands() else np.full(image.size[::-1], 255)
                selected = np.asarray(mask) > 0
                run = {
                    "iteration": iteration, "seconds": elapsed,
                    "peak_gpu_allocated_bytes": torch.cuda.max_memory_allocated(),
                    "peak_gpu_reserved_bytes": torch.cuda.max_memory_reserved(),
                    "native_mode": native.mode,
                    "native_min_alpha": int(alpha.min()),
                    "native_target_zero_alpha_pixels": int((alpha[selected] == 0).sum()),
                    "outside_raw_mean_absolute_8bit_channel_change": float(delta[outside].mean()) if outside.any() else None,
                    "outside_raw_p95_absolute_8bit_channel_change": float(np.percentile(delta[outside], 95)) if outside.any() else None,
                    "outside_composed_max_absolute_8bit_channel_change": int(composed_delta[outside].max()) if outside.any() else None,
                }
                for label, result in (("native", native), ("raw", raw), ("composed", composed)):
                    path = output / f"{iteration}-{label}.png"
                    result.save(path)
                    run[label + "_sha256"] = digest(path)
                record["runs"].append(run)
                (args.output / "measurement.json").write_text(json.dumps(manifest, indent=2) + "\n")
                print(json.dumps({"case": case["name"], **run}), flush=True)


if __name__ == "__main__":
    main()
