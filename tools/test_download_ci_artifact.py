import hashlib
import io
import tempfile
import unittest
import urllib.error
from pathlib import Path
from unittest.mock import patch

from download_ci_artifact import download


class DownloadChecks(unittest.TestCase):
    def test_transient_http_failure_retries_and_publishes_verified_bytes(self):
        payload = b"pinned model fixture"
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "weights.bin"
            failure = urllib.error.HTTPError("https://fixture", 503, "Unavailable", {}, None)
            with patch("download_ci_artifact.urllib.request.urlopen", side_effect=[failure, io.BytesIO(payload)]) as request, patch("download_ci_artifact.time.sleep"):
                download("https://fixture", output, hashlib.sha256(payload).hexdigest(), len(payload))
            self.assertEqual(request.call_count, 2)
            self.assertEqual(output.read_bytes(), payload)
            self.assertEqual(list(Path(directory).iterdir()), [output])

    def test_integrity_failure_is_never_retried_or_published(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "weights.bin"
            with patch("download_ci_artifact.urllib.request.urlopen", return_value=io.BytesIO(b"corrupt")) as request:
                with self.assertRaises(ValueError):
                    download("https://fixture", output, "0" * 64, 7)
            self.assertEqual(request.call_count, 1)
            self.assertEqual(list(Path(directory).iterdir()), [])

    def test_exhausted_retry_and_existing_destination_remain_safe(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "weights.bin"
            failure = urllib.error.HTTPError("https://fixture", 503, "Unavailable", {}, None)
            with patch("download_ci_artifact.urllib.request.urlopen", side_effect=failure) as request, patch("download_ci_artifact.time.sleep"):
                with self.assertRaises(urllib.error.HTTPError):
                    download("https://fixture", output, "0" * 64)
            self.assertEqual(request.call_count, 3)
            self.assertFalse(output.exists())
            output.write_bytes(b"existing")
            with patch("download_ci_artifact.urllib.request.urlopen") as request:
                with self.assertRaises(FileExistsError):
                    download("https://fixture", output, "0" * 64)
            request.assert_not_called()
            self.assertEqual(output.read_bytes(), b"existing")


if __name__ == "__main__":
    unittest.main()
