#!/usr/bin/env python3
"""Check that prepared SAM tensor factories follow the inference device."""
import tempfile
import unittest
from pathlib import Path
import torch
from export_sam3 import portable_devices


class Factories(torch.nn.Module):
    def forward(self, x):
        return x + torch.arange(x.shape[-1], device=x.device)


class PortableDevices(unittest.TestCase):
    def test_factories_follow_input_after_save_and_cpu_reload(self):
        device = "cuda" if torch.cuda.is_available() else "cpu"
        x = torch.zeros(1, 8, device=device)
        model = torch.jit.freeze(torch.jit.trace(Factories().eval(), x))
        model = portable_devices(model)
        self.assertIn("prim::device", str(model.graph))
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "factories.pt"
            model.save(str(path))
            cpu = torch.jit.load(str(path), map_location="cpu")
            self.assertTrue(torch.equal(cpu(x.cpu()), torch.arange(8).reshape(1, 8)))


if __name__ == "__main__":
    unittest.main()
