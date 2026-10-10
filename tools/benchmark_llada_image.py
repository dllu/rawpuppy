#!/usr/bin/env python3
"""Evaluate the pinned September 2026 LLaDA-Image-Turbo reference editor.

The publisher API has no mask input. Its full edited output is retained, then
composited through the supplied target mask. This is a comparison, not an app backend.
"""

import argparse
import hashlib
import importlib.metadata
import json
import os
import subprocess
import sys
import time
from pathlib import Path

import numpy as np
import torch
from PIL import Image


MODEL_REPOSITORY = "inclusionAI/LLaDA-Image-Turbo"
MODEL_REVISION = "f4afc52d925bbac4e22a1c947111fc1f127e37e5"
SOURCE_REVISION = "e7c861b0aaa00d2f7ed49600a3a6f170e02a9d59"


def digest(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def verify_snapshot(directory, metadata_path):
    metadata = json.loads(metadata_path.read_text())
    if metadata["sha"] != MODEL_REVISION or metadata["id"] != MODEL_REPOSITORY:
        raise ValueError("Unexpected model snapshot identity")
    hashes = {}
    total = 0
    for item in metadata["siblings"]:
        name = item["rfilename"]
        if name.startswith("assets/") or name == ".gitattributes":
            continue
        path = directory / name
        if not path.is_file() or path.stat().st_size != item["size"]:
            raise ValueError(f"Missing or wrong-size model file: {name}")
        sha = digest(path)
        if item.get("lfs"):
            if sha != item["lfs"]["sha256"]:
                raise ValueError(f"Checkpoint checksum mismatch: {name}")
        else:
            blob = hashlib.sha1(
                b"blob " + str(item["size"]).encode() + b"\0" + path.read_bytes()
            ).hexdigest()
            if blob != item["blobId"]:
                raise ValueError(f"Configuration/source checksum mismatch: {name}")
        hashes[name] = sha
        total += item["size"]
    return hashes, total


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--metadata", type=Path, required=True)
    parser.add_argument("--image", type=Path, required=True)
    parser.add_argument("--mask", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--prompt", required=True)
    parser.add_argument("--steps", type=int, default=4)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--iterations", type=int, default=2)
    parser.add_argument("--cuda-budget-gib", type=float, default=58.0)
    parser.add_argument("--deterministic-scheduler", action="store_true")
    args = parser.parse_args()
    if args.steps < 1 or args.iterations < 1 or not 0 < args.cuda_budget_gib <= 64:
        parser.error("Use positive steps/iterations and a CUDA budget up to 64 GiB")
    if args.output.exists():
        parser.error("Choose a fresh output directory")
    image = Image.open(args.image).convert("RGB")
    mask = Image.open(args.mask).convert("L")
    if image.size != mask.size or any(d % 32 for d in image.size):
        parser.error("Provide matching image/mask dimensions divisible by 32")
    if not np.asarray(mask).any():
        parser.error("The target selection is empty")
    revision = subprocess.check_output(
        ["git", "-C", str(args.source), "rev-parse", "HEAD"], text=True
    ).strip()
    dirty = subprocess.check_output(
        ["git", "-C", str(args.source), "status", "--porcelain"], text=True
    ).strip()
    if revision != SOURCE_REVISION or dirty:
        parser.error("Use the unchanged pinned publisher source checkout")
    hashes, model_bytes = verify_snapshot(args.model, args.metadata)
    if not torch.cuda.is_available():
        parser.error("This measurement requires CUDA")
    total_memory = torch.cuda.get_device_properties(0).total_memory
    fraction = min(args.cuda_budget_gib * 1024**3 / total_memory, 1.0)
    torch.cuda.set_per_process_memory_fraction(fraction, 0)
    torch.set_num_threads(8)
    torch.set_num_interop_threads(1)
    sys.path.insert(0, str(args.source))
    from src import LLaDAImagePipeline

    versions = {
        name: importlib.metadata.version(name)
        for name in ("torch", "transformers", "diffusers", "accelerate", "huggingface_hub")
    }
    original_hash = digest(args.image)
    mask_hash = digest(args.mask)
    args.output.mkdir(parents=True)
    record = {
        "purpose": "recent reference editing with explicit final target composition",
        "model_repository": MODEL_REPOSITORY,
        "model_revision": MODEL_REVISION,
        "source_revision": revision,
        "model_files_sha256": hashes,
        "model_bytes": model_bytes,
        "versions": versions,
        "device": torch.cuda.get_device_name(0),
        "dtype": "bfloat16",
        "cuda_budget_gib": args.cuda_budget_gib,
        "prompt": args.prompt,
        "dimensions": list(image.size),
        "steps": args.steps,
        "seed": args.seed,
        "guidance_scale": 1.0,
        "image_sha256": original_hash,
        "mask_sha256": mask_hash,
        "conditioning": "single original reference image plus editing prompt; no mask_image API",
        "composition": "supplied target mask only; unselected pixels are exact",
        "moe_backend_override": os.environ.get("LLADA_MOE_BACKEND"),
        "rng_policy": "Reset CPU and CUDA global RNGs and the per-call CUDA generator before each iteration",
        "runs": [],
    }
    measurement = args.output / "measurement.json"
    measurement.write_text(json.dumps(record, indent=2) + "\n")
    started = time.monotonic()
    pipe = LLaDAImagePipeline.from_pretrained(
        args.model, torch_dtype=torch.bfloat16, device="cuda"
    )
    torch.cuda.synchronize()
    record["load_seconds"] = time.monotonic() - started
    if args.deterministic_scheduler:
        pipe.scheduler.register_to_config(stochastic_sampling=False)
    record["scheduler"] = dict(pipe.scheduler.config)
    print(json.dumps({"loaded": True, "seconds": record["load_seconds"]}), flush=True)
    original = np.asarray(image, dtype=np.uint8)
    selection = np.asarray(mask, dtype=np.float32) / 255.0
    for iteration in range(args.iterations):
        # The publisher's stochastic scheduler calls step() without forwarding
        # its per-call generator. Seed that global RNG as well as initial latents.
        torch.manual_seed(args.seed)
        torch.cuda.manual_seed_all(args.seed)
        torch.cuda.reset_peak_memory_stats()
        started = time.monotonic()
        output = pipe(
            prompt=args.prompt,
            image=image,
            generation_mode="editing",
            height=image.height,
            width=image.width,
            num_inference_steps=args.steps,
            guidance_scale=1.0,
            generator=torch.Generator("cuda").manual_seed(args.seed),
        ).images[0].convert("RGB")
        torch.cuda.synchronize()
        seconds = time.monotonic() - started
        raw = np.asarray(output, dtype=np.uint8)
        if raw.shape != original.shape:
            raise ValueError("The publisher returned different output dimensions")
        composed = np.round(
            original.astype(np.float32)
            + selection[..., None] * (raw.astype(np.float32) - original)
        ).clip(0, 255).astype(np.uint8)
        outside = selection == 0
        if not np.array_equal(composed[outside], original[outside]):
            raise ValueError("Unselected pixels changed")
        raw_path = args.output / f"{iteration}-raw.png"
        composed_path = args.output / f"{iteration}-composed.png"
        output.save(raw_path)
        Image.fromarray(composed).save(composed_path)
        outside_delta = np.abs(raw.astype(np.int16) - original.astype(np.int16))[outside]
        run = {
            "iteration": iteration,
            "seconds": seconds,
            "peak_gpu_allocated_bytes": torch.cuda.max_memory_allocated(),
            "peak_gpu_reserved_bytes": torch.cuda.max_memory_reserved(),
            "raw_sha256": digest(raw_path),
            "composed_sha256": digest(composed_path),
            "outside_raw_mean_absolute_8bit_change": float(outside_delta.mean()) if outside_delta.size else 0.0,
            "outside_raw_p95_absolute_8bit_change": float(np.percentile(outside_delta, 95)) if outside_delta.size else 0.0,
            "outside_composed_exact": True,
        }
        record["runs"].append(run)
        measurement.write_text(json.dumps(record, indent=2) + "\n")
        print(json.dumps(run), flush=True)
    if digest(args.image) != original_hash or digest(args.mask) != mask_hash:
        raise ValueError("Comparison input or mask changed")


if __name__ == "__main__":
    main()
