"""Command line for writing a strategy bundle's dependency lock.

``python -m follon_strategy_sdk.bundle_lock --bundle-root <dir> --strategy-file
<file> --class-name <Class> --output <new file>`` writes the lock and prints the
bundle hash it describes. The output file must not already exist.
"""

from __future__ import annotations

from argparse import ArgumentParser
import json
from pathlib import Path
import sys

from .bundle import strategy_bundle_lock


def main(argv: list[str] | None = None) -> int:
    """Writes a bundle's dependency lock to a new file."""

    parser = ArgumentParser(description="Write a Follon strategy bundle dependency lock")
    parser.add_argument("--bundle-root", required=True, type=Path)
    parser.add_argument("--strategy-file", required=True, type=Path)
    parser.add_argument("--class-name", required=True)
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args(argv)
    try:
        contents = strategy_bundle_lock(
            arguments.bundle_root, arguments.strategy_file, arguments.class_name
        )
        with arguments.output.open("xb") as handle:
            handle.write(contents)
    except (OSError, ValueError) as error:
        print(f"strategy bundle lock failed: {error}", file=sys.stderr)
        return 2
    print(json.loads(contents)["strategy_bundle_hash"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
