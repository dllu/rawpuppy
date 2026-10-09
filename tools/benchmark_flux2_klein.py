#!/usr/bin/env python3
"""Evaluate Apache-2.0 FLUX.2 klein 4B masked editing outside the application.

Use an isolated environment with the official Flux2KleinInpaintPipeline. Download
the Diffusers layout at the pinned revision first; no weights are redistributed.
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
from diffusers import Flux2KleinInpaintPipeline
from PIL import Image, ImageFilter


MODEL_REPOSITORY = "black-forest-labs/FLUX.2-klein-4B"
MODEL_REVISION = "e7b7dc27f91deacad38e78976d1f2b499d76a294"


def digest(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def model_identity(root):
    hashes = {}
    for path in sorted(root.rglob("*.safetensors")):
        relative = path.relative_to(root)
        metadata = root / ".cache/huggingface/download" / f"{relative}.metadata"
        lines = metadata.read_text().splitlines()
        if len(lines) < 2 or lines[0] != MODEL_REVISION or len(lines[1]) != 64:
            raise ValueError(f"Missing pinned checksum metadata: {relative}")
        actual = digest(path)
        if actual != lines[1]:
            raise ValueError(f"Checkpoint checksum mismatch: {relative}")
        hashes[str(relative)] = actual
    if len(hashes) != 4:
        raise ValueError("Expected four safetensors files in the Diffusers layout")
    return hashes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--image", type=Path, required=True)
    parser.add_argument("--mask", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--prompt", required=True)
    parser.add_argument("--width", type=int, default=512)
    parser.add_argument("--height", type=int, default=512)
    parser.add_argument("--steps", type=int, default=4)
    parser.add_argument("--strength", type=float, default=1.0)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--iterations", type=int, default=2)
    parser.add_argument("--mask-padding", type=int, default=0,
                        help="Dilate the inference mask in model pixels; final composition uses the original mask")
    args = parser.parse_args()
    if min(args.steps, args.width, args.height, args.iterations) <= 0:
        parser.error("Steps, dimensions, and iterations must be positive")
    if args.width % 16 or args.height % 16:
        parser.error("Output dimensions must be multiples of 16")
    if args.width * args.height > 1024 * 1024:
        parser.error("The upstream pipeline caps input at one megapixel; use a bounded context")
    if not 0 <= args.mask_padding <= max(args.width, args.height):
        parser.error("Mask padding must be nonnegative and no larger than the context")
    if not 0 < args.strength <= 1 or int(args.steps * args.strength) < 1:
        parser.error("Strength must retain at least one step and be at most one")
    image = Image.open(args.image).convert("RGB")
    mask = Image.open(args.mask).convert("L")
    if image.size != mask.size:
        parser.error("Image and mask must have identical dimensions")
    if args.width * image.height != args.height * image.width:
        parser.error("Choose output dimensions with the original aspect ratio")
    if not np.asarray(mask).any():
        parser.error("The mask must select at least one pixel")
    if not torch.cuda.is_available():
        parser.error("This measurement requires a CUDA device")
    if args.output.exists():
        parser.error("Choose a new output directory to retain previous results")
    # Upstream uses the image's dimensions rather than width/height when an
    # image is supplied. Resize explicitly and record it instead of measuring
    # an unintended output grid. Do not stretch the photographic context.
    model_image = image.resize((args.width, args.height), Image.Resampling.LANCZOS)
    model_mask = mask.resize((args.width, args.height), Image.Resampling.NEAREST)
    if args.mask_padding:
        model_mask = model_mask.filter(ImageFilter.MaxFilter(2 * args.mask_padding + 1))
    # Validate checkpoint identity before loading large GPU allocations. This
    # reads pinned download metadata; it does not execute model-provided code.
    hashes = model_identity(args.model)
    distribution = importlib.metadata.distribution("diffusers")
    direct_url = json.loads(distribution.read_text("direct_url.json") or "{}")
    versions = {
        name: importlib.metadata.version(name)
        for name in ("torch", "diffusers", "transformers", "accelerate")
    }
    torch.set_num_threads(8)
    torch.set_num_interop_threads(1)
    args.output.mkdir(parents=True)
    manifest = {
        "purpose": "local masked-editing comparison; no application integration",
        "model_repository": MODEL_REPOSITORY,
        "model_revision": MODEL_REVISION,
        "model_license": "Apache-2.0",
        "model_file_sha256": hashes,
        "pipeline_class": "Flux2KleinInpaintPipeline",
        "pipeline_direct_url": direct_url,
        "image_sha256": digest(args.image),
        "mask_sha256": digest(args.mask),
        "input_size": image.size,
        "requested_output_size": [args.width, args.height],
        "model_input_size": model_image.size,
        "input_resize": "aspect-preserving Lanczos RGB and nearest-neighbour mask",
        "prompt": args.prompt,
        "versions": versions,
        "device": torch.cuda.get_device_name(),
        "dtype": "bfloat16",
        "mask_input": "native mask_image; white repaints, black preserves",
        "model_mask_padding_pixels": args.mask_padding,
        "composition_mask": "original mask, without inference padding",
        "reference_input": "pipeline conditions on original image latents",
        "requested_steps": args.steps,
        "strength": args.strength,
        "seed": args.seed,
        "guidance_scale": 1.0,
        "loader": "CPU load followed by to(cuda); no compilation or offload",
        "runs": [],
    }
    started = time.perf_counter()
    pipe = Flux2KleinInpaintPipeline.from_pretrained(
        args.model, torch_dtype=torch.bfloat16, local_files_only=True,
        low_cpu_mem_usage=True,
    ).to("cuda")
    torch.cuda.synchronize()
    manifest["load_seconds"] = time.perf_counter() - started
    print(json.dumps({"load_seconds": manifest["load_seconds"], "versions": versions}), flush=True)
    with torch.inference_mode():
        for iteration in range(args.iterations):
            torch.cuda.reset_peak_memory_stats()
            started = time.perf_counter()
            native = pipe(
                prompt=args.prompt, image=model_image, mask_image=model_mask,
                width=args.width, height=args.height,
                num_inference_steps=args.steps, strength=args.strength,
                guidance_scale=1.0,
                generator=torch.Generator("cuda").manual_seed(args.seed),
            ).images[0]
            torch.cuda.synchronize()
            elapsed = time.perf_counter() - started
            if native.size != (args.width, args.height):
                raise ValueError(f"Unexpected native output grid: {native.size}")
            raw = native.convert("RGB")
            # Retain the native grid; metrics and composition use the common
            # original grid. This measures mask preservation, not fill quality.
            aligned = raw.resize(image.size, Image.Resampling.LANCZOS)
            composed = Image.composite(aligned, image, mask)
            outside = np.asarray(mask) == 0
            original = np.asarray(image).astype(np.int16)
            delta = np.abs(np.asarray(aligned).astype(np.int16) - original)
            composed_delta = np.abs(np.asarray(composed).astype(np.int16) - original)
            run = {
                "iteration": iteration,
                "seconds": elapsed,
                "peak_gpu_allocated_bytes": torch.cuda.max_memory_allocated(),
                "peak_gpu_reserved_bytes": torch.cuda.max_memory_reserved(),
                "process_peak_rss_bytes": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
                * (1 if sys.platform == "darwin" else 1024),
                "executed_steps": pipe.num_timesteps,
                "output_size": raw.size,
                "native_mode": native.mode,
                "outside_raw_mean_absolute_8bit_channel_change": float(delta[outside].mean())
                if outside.any() else None,
                "outside_raw_p95_absolute_8bit_channel_change": float(np.percentile(delta[outside], 95))
                if outside.any() else None,
                "outside_composed_max_absolute_8bit_channel_change": int(composed_delta[outside].max())
                if outside.any() else None,
            }
            outputs = {
                "native": native,
                "raw": raw,
                "composed": composed,
            }
            run["output_sha256"] = {}
            for name, output in outputs.items():
                path = args.output / f"{iteration}-{name}.png"
                output.save(path)
                run["output_sha256"][path.name] = digest(path)
            manifest["runs"].append(run)
            (args.output / "measurement.json").write_text(json.dumps(manifest, indent=2) + "\n")
            print(json.dumps(run), flush=True)


if __name__ == "__main__":
    main()
