"""Bounded transient retries for pinned CI model artifacts."""
import hashlib
import http.client
import os
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path


def download(url, destination, expected_sha256, expected_bytes=None):
    destination = Path(destination)
    if destination.exists():
        raise FileExistsError(destination)
    for attempt in range(3):
        temporary = None
        try:
            with urllib.request.urlopen(url, timeout=60) as response:
                with tempfile.NamedTemporaryFile(dir=destination.parent, delete=False) as target:
                    temporary = Path(target.name)
                    hasher = hashlib.sha256()
                    length = 0
                    while chunk := response.read(1024 * 1024):
                        length += len(chunk)
                        if expected_bytes is not None and length > expected_bytes:
                            raise ValueError("Published checkpoint has unexpected size")
                        hasher.update(chunk)
                        target.write(chunk)
            if (expected_bytes is not None and length != expected_bytes) or hasher.hexdigest() != expected_sha256:
                raise ValueError("Published checkpoint identity mismatch")
            # Fresh CI scratch directory; link publishes without clobbering an
            # existing destination, including an independently created file.
            os.link(temporary, destination)
            return
        except urllib.error.HTTPError as error:
            if error.code not in (408, 429, 500, 502, 503, 504) or attempt == 2:
                raise
        except (urllib.error.URLError, TimeoutError, ConnectionError, http.client.IncompleteRead):
            if attempt == 2:
                raise
        finally:
            if temporary is not None:
                temporary.unlink(missing_ok=True)
        time.sleep(2 ** attempt)
