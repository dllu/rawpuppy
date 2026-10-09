"""Serialized-graph precision and portability regression checks."""

import tempfile
import unittest
from pathlib import Path

import torch

from export_moebius import portable_scalar_constants


class Scale(torch.nn.Module):
    def __init__(self, value):
        super().__init__()
        self.register_buffer("scale", torch.tensor(value, dtype=torch.float64))

    def forward(self, image):
        return image * self.scale


class Portability(unittest.TestCase):
    def graph(self, value):
        sample = torch.tensor([-3.5, 0., 0.1, 3.], dtype=torch.float32)
        graph = torch.jit.freeze(torch.jit.trace(Scale(value).eval(), sample), optimize_numerics=False)
        return graph, sample

    def test_exact_scalar_survives_serialization_without_changing_results(self):
        graph, sample = self.graph(0.5)
        expected = graph(sample)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "graph.pt"
            graph.save(str(path))
            loaded = torch.jit.load(str(path))
            self.assertEqual(portable_scalar_constants(loaded), [0.5])
            loaded.save(str(path))
            final = torch.jit.load(str(path))
            self.assertEqual(portable_scalar_constants(final), [])
            with torch.jit.optimized_execution(False):
                self.assertTrue(torch.equal(final(sample), expected))
            self.assertEqual(final(sample).dtype, torch.float32)

    def test_inexact_scalar_is_rejected(self):
        graph, _ = self.graph(0.1)
        with self.assertRaisesRegex(ValueError, "nonportable"):
            portable_scalar_constants(graph)

    def test_non_scalar_is_rejected(self):
        graph, _ = self.graph([1.])
        with self.assertRaisesRegex(ValueError, "nonportable"):
            portable_scalar_constants(graph)


if __name__ == "__main__":
    unittest.main()
