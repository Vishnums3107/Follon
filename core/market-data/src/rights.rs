//! Data-rights and semantic-drift ledger (DUR-03, DATA-01).
//!
//! Tracks market data provider licenses, redistribution permissions, point-in-time
//! corporate-action adjustment policies, and cross-provider semantic parity.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::MarketDataError;

/// Legal license tier governing data usage.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LicenseTier {
    /// Restricted to private internal research; no live trading or redistribution.
    InternalResearchOnly,
    /// Licensed for simulation, backtest, and deterministic replay.
    CommercialReplay,
    /// Enterprise license permitting commercial distribution and multi-tenant replay.
    EnterpriseRedistributable,
}

impl LicenseTier {
    /// Returns the canonical uppercase representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InternalResearchOnly => "INTERNAL_RESEARCH_ONLY",
            Self::CommercialReplay => "COMMERCIAL_REPLAY",
            Self::EnterpriseRedistributable => "ENTERPRISE_REDISTRIBUTABLE",
        }
    }
}

/// Point-in-time corporate action handling policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CorporateActionPolicy {
    /// Both stock splits and cash dividends back-adjusted into prices.
    RawSplitAndDividendAdjusted,
    /// Unadjusted raw exchange trade prices.
    RawUnadjusted,
    /// Only stock splits adjusted; cash dividends retained as cash events.
    SplitAdjustedOnly,
}

impl CorporateActionPolicy {
    /// Returns the canonical uppercase representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RawSplitAndDividendAdjusted => "RAW_SPLIT_AND_DIVIDEND_ADJUSTED",
            Self::RawUnadjusted => "RAW_UNADJUSTED",
            Self::SplitAdjustedOnly => "SPLIT_ADJUSTED_ONLY",
        }
    }
}

/// Data-rights and semantics verification receipt matching `data-rights-and-semantics-receipt.schema.json`.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct DataRightsAndSemanticsReceipt {
    /// Schema version (fixed at 1).
    pub receipt_schema_version: u32,
    /// Unique receipt identifier.
    pub receipt_id: String,
    /// Provider identifier (e.g., "databento", "polygon", "ibkr").
    pub provider_id: String,
    /// Bound dataset identifier.
    pub dataset_id: String,
    /// Certified license tier.
    pub license_tier: LicenseTier,
    /// Whether external redistribution is permitted.
    pub redistribution_permitted: bool,
    /// Explicit corporate-action adjustment policy.
    pub corporate_action_policy: CorporateActionPolicy,
    /// Semantic parity score across providers in basis points (10000 = 100.00% parity).
    pub semantic_parity_score_bps: u32,
    /// RFC3339 verification timestamp.
    pub verified_at: String,
    /// RFC3339 expiration timestamp.
    pub expires_at: String,
}

/// Ledger maintaining data entitlements, license compliance, and semantic parity.
pub struct DataRightsLedger;

impl DataRightsLedger {
    /// Certifies rights and generates an immutable receipt.
    pub fn certify_receipt(
        provider_id: &str,
        dataset_id: &str,
        license_tier: LicenseTier,
        corporate_action_policy: CorporateActionPolicy,
        semantic_parity_score_bps: u32,
        verified_at: &str,
        expires_at: &str,
    ) -> Result<DataRightsAndSemanticsReceipt, MarketDataError> {
        if semantic_parity_score_bps > 10_000 {
            return Err(MarketDataError(
                "semantic parity score cannot exceed 10000 basis points".to_owned(),
            ));
        }

        let redistribution_permitted = license_tier == LicenseTier::EnterpriseRedistributable;

        let digest = format!(
            "{:x}",
            Sha256::digest(format!("{}:{}:{}", provider_id, dataset_id, verified_at).as_bytes())
        );
        let receipt_id = format!("drsr.{}", &digest[..16]);

        Ok(DataRightsAndSemanticsReceipt {
            receipt_schema_version: 1,
            receipt_id,
            provider_id: provider_id.to_owned(),
            dataset_id: dataset_id.to_owned(),
            license_tier,
            redistribution_permitted,
            corporate_action_policy,
            semantic_parity_score_bps,
            verified_at: verified_at.to_owned(),
            expires_at: expires_at.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certifies_commercial_replay_receipt() {
        let receipt = DataRightsLedger::certify_receipt(
            "databento",
            "ds.us_equities.minute.2026",
            LicenseTier::CommercialReplay,
            CorporateActionPolicy::SplitAdjustedOnly,
            9_995,
            "2026-09-01T12:00:00Z",
            "2027-09-01T12:00:00Z",
        )
        .unwrap();

        assert_eq!(receipt.receipt_schema_version, 1);
        assert!(!receipt.redistribution_permitted);
        assert_eq!(receipt.license_tier, LicenseTier::CommercialReplay);
        assert_eq!(receipt.semantic_parity_score_bps, 9_995);
    }
}
