#!/usr/bin/env python3
"""Research-only Qwen Image 2.1 mask/reference evaluation, outside the application.

Use an isolated environment with the official QwenImage21Pipeline. The model's
research license applies; this script neither installs nor redistributes weights.
"""

import argparse
import hashlib
import importlib.metadata
import json
import resource
import sys
import time
from pathlib import Path

import numpy as np
import torch
from diffusers import QwenImage21Pipeline
from PIL import Image


def digest(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def model_identity(root):
    hashes = {}
    for path in sorted(root.rglob("*.safetensors")):
        relative = path.relative_to(root)
        actual = digest(path)
        metadata = root / ".cache/huggingface/download" / f"{relative}.metadata"
        if metadata.exists():
            expected = metadata.read_text().splitlines()[1]
            if len(expected) == 64 and actual != expected:
                raise ValueError(f"Checkpoint checksum mismatch: {relative}")
        hashes[str(relative)] = actual
    return hashes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--image", type=Path, required=True)
    parser.add_argument("--mask", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--prompt", required=True)
    parser.add_argument("--resolution", type=int, default=512)
    parser.add_argument("--steps", type=int, default=40)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--iterations", type=int, default=1)
    parser.add_argument("--load-directly-on-cuda", action="store_true")
    args = parser.parse_args()
    if min(args.steps, args.resolution, args.iterations) <= 0:
        parser.error("Steps, resolution, and iterations must be positive")
    image = Image.open(args.image).convert("RGB")
    mask = Image.open(args.mask).convert("L")
    if image.size != mask.size:
        parser.error("Image and mask must have identical dimensions")
    if not torch.cuda.is_available():
        parser.error("This measurement requires a CUDA device")
    if (args.output / "measurement.json").exists():
        parser.error("Choose a new output directory to retain previous measurements")
    torch.set_num_threads(8)
    torch.set_num_interop_threads(1)
    args.output.mkdir(parents=True, exist_ok=True)
    metadata = args.model / ".cache/huggingface/download/model_index.json.metadata"
    model_revision = metadata.read_text().splitlines()[0] if metadata.exists() else None
    distribution = importlib.metadata.distribution("diffusers")
    direct_url = json.loads(distribution.read_text("direct_url.json") or "{}")
    versions = {
        name: importlib.metadata.version(name)
        for name in ("torch", "diffusers", "transformers", "accelerate")
    }
    manifest = {
        "purpose": "research/evaluation only; Qwen research license",
        "model": str(args.model.resolve()),
        "model_revision_from_download_metadata": model_revision,
        "model_file_sha256": model_identity(args.model),
        "pipeline_direct_url": direct_url,
        "image_sha256": digest(args.image),
        "mask_sha256": digest(args.mask),
        "prompt": args.prompt,
        "versions": versions,
        "device": torch.cuda.get_device_name(),
        "dtype": "bfloat16",
        "reference_order": ["original RGB image", "white-on-black mask RGB image"],
        "steps": args.steps,
        "seed": args.seed,
        "output_resolution": args.resolution,
        "use_kv_cache": True,
        "true_cfg_scale": 1.0,
        "loader": "device_map=cuda" if args.load_directly_on_cuda else "CPU load followed by to(cuda)",
        "runs": [],
    }
    started = time.perf_counter()
    options = {"device_map": "cuda"} if args.load_directly_on_cuda else {}
    pipe = QwenImage21Pipeline.from_pretrained(
        args.model, dtype=torch.bfloat16, local_files_only=True,
        low_cpu_mem_usage=True, **options,
    )
    if not args.load_directly_on_cuda:
        pipe = pipe.to("cuda")
    torch.cuda.synchronize()
    manifest["load_seconds"] = time.perf_counter() - started
    print(json.dumps({"load_seconds": manifest["load_seconds"], "versions": versions}), flush=True)
    with torch.inference_mode():
        for iteration in range(args.iterations):
            torch.cuda.reset_peak_memory_stats()
            started = time.perf_counter()
            native_result = pipe(
                prompt=args.prompt,
                image=[image, mask.convert("RGB")],
                num_inference_steps=args.steps,
                output_resolution=args.resolution,
                generator=torch.Generator("cuda").manual_seed(args.seed),
                true_cfg_scale=1.0,
                use_kv_cache=True,
            ).images[0]
            torch.cuda.synchronize()
            elapsed = time.perf_counter() - started
            result = native_result.convert("RGB")
            # Evaluate the aligned original grid, retaining the untouched native
            # result as well. A 1024 output is explicitly downsampled to the same
            # 512 reference for comparison; it is not a full-resolution RAW edit.
            aligned = result.resize(image.size, Image.Resampling.LANCZOS)
            original = np.asarray(image)
            selection = np.asarray(mask)
            raw_delta = np.abs(np.asarray(aligned).astype(np.int16) - original)
            outside = selection == 0
            composed = Image.composite(aligned, image, mask)
            composed_delta = np.abs(np.asarray(composed).astype(np.int16) - original)
            native_alpha = np.asarray(native_result.getchannel("A")) if "A" in native_result.getbands() else None
            native_selection = np.asarray(mask.resize(result.size, Image.Resampling.NEAREST)) > 0
            run = {
                "iteration": iteration,
                "seconds": elapsed,
                "peak_gpu_allocated_bytes": torch.cuda.max_memory_allocated(),
                "peak_gpu_reserved_bytes": torch.cuda.max_memory_reserved(),
                "process_peak_rss_bytes": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
                * (1 if sys.platform == "darwin" else 1024),
                "output_size": result.size,
                "native_mode": native_result.mode,
                "native_min_alpha": int(native_alpha.min()) if native_alpha is not None else 255,
                "native_target_min_alpha": int(native_alpha[native_selection].min())
                if native_alpha is not None and native_selection.any() else None,
                "native_target_zero_alpha_pixels": int((native_alpha[native_selection] == 0).sum())
                if native_alpha is not None else 0,
                "outside_raw_mean_absolute_8bit_channel_change": float(raw_delta[outside].mean())
                if outside.any() else None,
                "outside_raw_p95_absolute_8bit_channel_change": float(np.percentile(raw_delta[outside], 95))
                if outside.any() else None,
                "outside_composed_max_absolute_8bit_channel_change": int(composed_delta[outside].max())
                if outside.any() else None,
            }
            manifest["runs"].append(run)
            native_result.save(args.output / f"{iteration}-native.png")
            result.save(args.output / f"{iteration}-raw.png")
            composed.save(args.output / f"{iteration}-composed.png")
            (args.output / "measurement.json").write_text(json.dumps(manifest, indent=2) + "\n")
            print(json.dumps(run), flush=True)


if __name__ == "__main__":
    main()
