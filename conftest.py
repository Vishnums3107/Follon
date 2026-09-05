"""Root pytest configuration for Follon.

Ensures that all local Python packages and tools are discoverable without requiring manual
PYTHONPATH manipulation in CI or local environments.
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent

PACKAGE_DIRS = [
    ROOT / "python" / "strategy-sdk" / "src",
    ROOT / "python" / "storage-adapter" / "src",
    ROOT / "python" / "ibkr-gateway" / "src",
    ROOT / "tools",
]

for package_dir in PACKAGE_DIRS:
    path_str = str(package_dir)
    if path_str not in sys.path:
        sys.path.insert(0, path_str)
