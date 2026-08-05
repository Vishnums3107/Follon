"""Immutable research provenance shared by reproducible strategy runs."""

from __future__ import annotations

from dataclasses import dataclass
from hashlib import sha256

from .models import _canonical_id, _utc


BACKTEST_PROVENANCE_VERSION = 2


def _sha256(name: str, value: str) -> str:
    if len(value) != 64 or any(character not in "0123456789abcdef" for character in value):
        raise ValueError(f"{name} must be a lowercase SHA-256 digest")
    return value


@dataclass(frozen=True, slots=True)
class DatasetReference:
    """Versioned, content-addressed normalized input selected for research."""

    dataset_id: str
    dataset_version: str
    reference_data_version: str
    universe_id: str
    content_hash: str
    starts_at: str
    ends_at: str

    def __post_init__(self) -> None:
        _canonical_id("dataset_id", self.dataset_id)
        _canonical_id("universe_id", self.universe_id)
        _sha256("content_hash", self.content_hash)
        if not self.dataset_version or not self.reference_data_version:
            raise ValueError("dataset and reference-data versions are required")
        _utc("dataset starts_at", self.starts_at)
        _utc("dataset ends_at", self.ends_at)
        if self.starts_at > self.ends_at:
            raise ValueError("dataset time range must be ordered UTC")


@dataclass(frozen=True, slots=True)
class BacktestProvenance:
    """Input identity required before a strategy result can be called reproducible."""

    strategy_bundle_hash: str
    dataset: DatasetReference
    configuration_id: str
    configuration_version: str
    configuration_hash: str
    seed: int
    engine_version: str
    starts_at: str
    ends_at: str

    def __post_init__(self) -> None:
        _sha256("strategy_bundle_hash", self.strategy_bundle_hash)
        _canonical_id("configuration_id", self.configuration_id)
        _sha256("configuration_hash", self.configuration_hash)
        if self.seed < 0 or not self.configuration_version or not self.engine_version:
            raise ValueError("seed, configuration version, and engine version are required")
        _utc("backtest starts_at", self.starts_at)
        _utc("backtest ends_at", self.ends_at)
        if self.starts_at > self.ends_at:
            raise ValueError("backtest time range must be ordered UTC")
        if self.starts_at < self.dataset.starts_at or self.ends_at > self.dataset.ends_at:
            raise ValueError("backtest range must stay within the versioned dataset")

    def fingerprint(self) -> str:
        """Returns the Rust-compatible fingerprint of all declared run inputs.

        This line-oriented representation is a published cross-runtime
        contract. Its field labels and order must change only with a new
        provenance version; JSON object ordering is not an acceptable input to
        a reproducibility identity.
        """

        canonical = (
            f"provenance={BACKTEST_PROVENANCE_VERSION}\n"
            f"strategy={self.strategy_bundle_hash}\n"
            f"dataset_id={self.dataset.dataset_id}\n"
            f"dataset_version={self.dataset.dataset_version}\n"
            f"dataset_hash={self.dataset.content_hash}\n"
            f"reference_data={self.dataset.reference_data_version}\n"
            f"universe={self.dataset.universe_id}\n"
            f"config_id={self.configuration_id}\n"
            f"config_version={self.configuration_version}\n"
            f"config_hash={self.configuration_hash}\n"
            f"seed={self.seed}\n"
            f"engine={self.engine_version}\n"
            f"starts={self.starts_at}\n"
            f"ends={self.ends_at}\n"
        )
        return sha256(canonical.encode("utf-8")).hexdigest()
