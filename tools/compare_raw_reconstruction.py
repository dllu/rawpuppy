#!/usr/bin/env python3
"""Compare camera-linear reconstruction patches with an independent RAW capture.

Alignment is estimated from clean/noisy MHC green only. A single exposure ratio
from observed noisy CFA samples is shared by all methods, including the learned
result. Metrics use an interior, unsaturated reference region; this is not a
reproduction of the publisher's developed-image/MS-SSIM evaluation.
"""

import argparse
import hashlib
import json
import math
from pathlib import Path

import cv2
import numpy as np


def digest(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def patch(directory):
    metadata = json.loads((directory / "metadata.json").read_text())
    _, _, width, height = metadata["crop_xywh"]
    camera_path = directory / "mhc.f32"
    mosaic_path = directory / "mosaic.f32"
    if digest(camera_path) != metadata["mhc_sha256"] or digest(mosaic_path) != metadata["mosaic_sha256"]:
        raise ValueError("Patch checksum mismatch")
    camera = np.fromfile(camera_path, dtype="<f4").reshape(height, width, 3)
    mosaic = np.fromfile(mosaic_path, dtype="<f4").reshape(height, width)
    return metadata, camera, mosaic


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reference", type=Path, required=True)
    parser.add_argument("--noisy", type=Path, required=True)
    parser.add_argument("--learned", type=Path, required=True)
    parser.add_argument("--baseline", type=Path, action="append", default=[])
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Choose a new output file")
    reference_metadata, reference, _ = patch(args.reference)
    noisy_metadata, noisy, mosaic = patch(args.noisy)
    if reference.shape != noisy.shape or reference_metadata["crop_xywh"] != noisy_metadata["crop_xywh"]:
        parser.error("Choose equal sensor-coordinate patches")
    height, width, _ = reference.shape
    if min(width, height) <= 64:
        parser.error("Metrics require an interior after a 32-pixel border")
    shift, response = cv2.phaseCorrelate(
        cv2.GaussianBlur(reference[:, :, 1].copy(), (9, 9), 2),
        cv2.GaussianBlur(noisy[:, :, 1].copy(), (9, 9), 2),
    )
    if response < 0.1 or max(abs(v) for v in shift) > 24:
        raise ValueError(f"Unreliable patch alignment: {shift}, {response}")
    # Warp only the clean reference. Interpolating the noisy image would itself
    # suppress noise and change the methods under comparison.
    aligned = cv2.warpAffine(reference, np.float32([[1, 0, shift[0]], [0, 1, shift[1]]]),
                             (width, height), flags=cv2.INTER_LINEAR,
                             borderMode=cv2.BORDER_CONSTANT, borderValue=(0, 0, 0))
    valid = np.zeros((height, width), dtype=bool)
    valid[32:-32, 32:-32] = True
    valid &= np.isfinite(aligned).all(2) & (aligned.max(2) < 0.99)
    if not valid.any():
        raise ValueError("No unsaturated reference region")
    # raw_patch exports RGGB contexts; map each location to its observed color.
    yy, xx = np.indices((height, width))
    channel = np.where(yy % 2 == 0, np.where(xx % 2 == 0, 0, 1),
                       np.where(xx % 2 == 0, 1, 2))
    clean_observed = aligned[yy, xx, channel]
    exposure = float(clean_observed[valid].sum(dtype=np.float64)
                     / mosaic[valid].sum(dtype=np.float64))
    if not math.isfinite(exposure) or exposure <= 0:
        raise ValueError("Invalid shared exposure ratio")
    methods = [("MHC", noisy, noisy_metadata["mhc_sha256"])]
    for directory in args.baseline:
        metadata, image, _ = patch(directory)
        if metadata["source_sha256"] != noisy_metadata["source_sha256"] or metadata["crop_xywh"] != noisy_metadata["crop_xywh"]:
            raise ValueError("Baseline source/context differs")
        methods.append((f"MHC + sensor bilateral sigma={metadata['raw_edits']['denoise']}",
                        image, metadata["mhc_sha256"]))
    measurement = json.loads((args.learned / "measurement.json").read_text())
    if measurement["source_sha256"] != noisy_metadata["source_sha256"] or measurement["crop_xywh"] != noisy_metadata["crop_xywh"]:
        raise ValueError("Learned source/context differs")
    learned_path = args.learned / "0-camera.f32"
    if digest(learned_path) != measurement["runs"][0]["camera_sha256"]:
        raise ValueError("Learned output checksum mismatch")
    learned = np.fromfile(learned_path, dtype="<f4").reshape(height, width, 3)
    methods.append(("RawNIND joint Bayer", learned, digest(learned_path)))
    results = []
    for name, image, sha in methods:
        values = image[valid].astype(np.float64) * exposure
        target = aligned[valid].astype(np.float64)
        error = values - target
        mse = float(np.square(error).mean())
        results.append({
            "method": name, "input_sha256": sha,
            "camera_linear_rmse": math.sqrt(mse),
            "camera_linear_mae": float(np.abs(error).mean()),
            "camera_linear_psnr_reference_peak_one_db": -10 * math.log10(mse) if mse > 0 else None,
            "per_channel_mean_ratio": (values.mean(0) / target.mean(0)).tolist(),
        })
    record = {
        "reference_source_sha256": reference_metadata["source_sha256"],
        "noisy_source_sha256": noisy_metadata["source_sha256"],
        "crop_xywh": noisy_metadata["crop_xywh"],
        "reference_alignment_to_noisy_xy": shift,
        "phase_correlation_response": response,
        "alignment_input": "Clean/noisy MHC green; Gaussian sigma 2, 9×9 support",
        "reference_resampling": "OpenCV linear interpolation, reference only",
        "shared_exposure_from_observed_cfa_samples": exposure,
        "excluded_border_pixels": 32,
        "valid_reference_pixels": int(valid.sum()),
        "saturation_exclusion": "Reference camera-RGB maximum < 0.99",
        "scope": "One ROI; clean reference itself uses MHC; not publisher metrics or a general quality ranking",
        "results": results,
    }
    args.output.write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps(record, indent=2))


if __name__ == "__main__":
    main()
