#!/usr/bin/env python3
"""Prepare SAM 3's image detector for native architectural perspective analysis.

Requires PyTorch and Transformers >=5.17, and access to facebook/sam3. Model
parameters are separate from Rawpuppy's source; the SAM license is copied beside
the prepared graph. No Python process is required by the editor.
"""
import argparse
import hashlib
import json
from pathlib import Path

import torch
from PIL import Image
from transformers import Sam3Model, Sam3Processor
from transformers.modeling_outputs import BaseModelOutputWithPooling
from transformers.models.sam3.modeling_sam3 import Sam3VisionEncoderOutput

REVISION = "3c879f39826c281e95690f02c7821c4de09afae7"
PROMPTS = ["building", "wall", "tree", "person"]
WEIGHTS_SHA256 = "6d06f0a5f84e435071fe6603e61d0b4cc7b40e0d39d487cfd4d67d8cc11cc14a"


def digest(path):
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def portable_devices(module):
    """Tracing records Tensor.device as a literal. Follow the input at runtime.

    map_location relocates tensor storage, but cannot relocate Device literals
    used by SAM's positional encoding and attention-mask tensor factories.
    """
    graph = module.graph
    pixels = list(graph.inputs())[1]
    device = graph.create("prim::device", [pixels])
    device.output().setType(torch._C.DeviceObjType.get())
    device.insertBefore(next(graph.nodes()))
    def visit(block):
        for node in list(block.nodes()):
            if node.kind() == "prim::Constant" and str(node.output().type()) == "Device":
                node.output().replaceAllUsesWith(device.output())
            for nested in node.blocks():
                visit(nested)
    visit(graph)
    torch._C._jit_pass_dce(graph)
    return module


class ArchitectureMask(torch.nn.Module):
    def __init__(self, model, processor):
        super().__init__()
        self.model = model
        tokens = processor(text=PROMPTS, padding=True, return_tensors="pt").to(model.device)
        with torch.no_grad():
            features = model.get_text_features(**tokens).pooler_output
        self.register_buffer("text_features", features)
        self.register_buffer("text_mask", tokens.attention_mask)

    def forward(self, pixels):
        vision = self.model.get_vision_features(pixel_values=pixels)
        # Share the expensive image encoder across the four concept prompts.
        batched = Sam3VisionEncoderOutput(
            fpn_hidden_states=tuple(x.expand(4, -1, -1, -1) for x in vision.fpn_hidden_states),
            fpn_position_encoding=tuple(x.expand(4, -1, -1, -1) for x in vision.fpn_position_encoding),
        )
        out = self.model(
            vision_embeds=batched,
            text_embeds=BaseModelOutputWithPooling(pooler_output=self.text_features),
            attention_mask=self.text_mask,
        )
        scores = out.pred_logits.sigmoid() * out.presence_logits.sigmoid()
        masks = out.pred_masks.sigmoid()
        # Keep concept confidence and pixel confidence separate, matching SAM's
        # instance filtering, rather than inventing edges from a soft mask.
        return (masks * (scores >= 0.5)[:, :, None, None]).amax(dim=1)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--image", type=Path, action="append", required=True)
    parser.add_argument("--device", default="cuda")
    args = parser.parse_args()
    if args.output.exists():
        parser.error("Choose a fresh output directory")
    if len(args.image)<3 or len({digest(p) for p in args.image})<3:
        parser.error("Supply at least three distinct validation photographs")
    from huggingface_hub import hf_hub_download
    weights=Path(hf_hub_download("facebook/sam3","model.safetensors",revision=REVISION))
    if digest(weights)!=WEIGHTS_SHA256:
        parser.error("SAM 3 weights differ from the pinned checkpoint")
    torch.set_num_threads(8)
    torch.set_num_interop_threads(1)
    torch.backends.cuda.matmul.allow_tf32 = False
    torch.backends.cudnn.allow_tf32 = False
    torch.backends.cudnn.benchmark = False
    model = Sam3Model.from_pretrained("facebook/sam3", revision=REVISION,
                                     attn_implementation="sdpa").to(args.device).eval()
    processor = Sam3Processor.from_pretrained("facebook/sam3", revision=REVISION)
    wrapper = ArchitectureMask(model, processor).eval()
    images = [Image.open(p).convert("RGB") for p in args.image]
    inputs = [processor(images=im, return_tensors="pt").pixel_values.to(args.device) for im in images]
    with torch.no_grad():
        print("Tracing fixed 1008-pixel SAM 3 image detector", flush=True)
        reference = [wrapper(x) for x in inputs]
        traced = torch.jit.trace(wrapper, inputs[0], check_trace=False, strict=False)
        traced = torch.jit.freeze(traced)
        traced = portable_devices(traced)
        checks = []
        for path, x, expected in zip(args.image, inputs, reference):
            actual = traced(x)
            error = float((actual - expected).abs().max())
            disagreements = int(((actual >= 0.5) != (expected >= 0.5)).sum())
            checks.append(dict(image=str(path), image_sha256=digest(path), max_absolute_error=error,
                               threshold_disagreements=disagreements))
            print(checks[-1], flush=True)
            if error > 1e-3 or disagreements:
                raise RuntimeError("Prepared SAM graph differs from the eager model")
        args.output.mkdir(parents=True)
        traced.save(str(args.output / "architecture.pt"))
        import urllib.request
        license_text = urllib.request.urlopen(
            "https://raw.githubusercontent.com/facebookresearch/sam3/main/LICENSE"
        ).read()
        (args.output / "LICENSE.txt").write_bytes(license_text)
        (args.output / "NOTICE.txt").write_text(
            "SAM 3: Segment Anything with Concepts, Meta, 2025.\n"
            "https://github.com/facebookresearch/sam3\n"
            "Checkpoint facebook/sam3, revision " + REVISION + "\n"
            "Model parameters and their derived graph are under the accompanying SAM License.\n"
        )
        manifest = dict(version=1, revision=REVISION, prompts=PROMPTS, portable_device=True,
                        input_edge=1008, mean=processor.image_processor.image_mean,
                        std=processor.image_processor.image_std,
                        weights_sha256=digest(weights),
                        graph_sha256=digest(args.output / "architecture.pt"), checks=checks)
        (args.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        for index, (im, masks) in enumerate(zip(images, reference)):
            valid = ((masks[:2].amax(0) >= 0.5) & (masks[2:].amax(0) < 0.5)).cpu()
            Image.fromarray((valid.numpy() * 255).astype("uint8")).resize(im.size).save(
                args.output / f"check-{index}-mask.png")
    print(args.output, flush=True)


if __name__ == "__main__":
    main()
