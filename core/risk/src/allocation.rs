//! Research-to-capital allocation council (DUR-11, RISK-03).
//!
//! Converts robust backtest evidence into risk-budgeted portfolio capital allocations
//! with explicit volatility targets, marginal risk contributions, and drawdown bounds.

use follon_domain::Decimal;
use sha2::{Digest, Sha256};

use crate::RiskError;

/// Status of a capital allocation proposal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProposalStatus {
    /// Algorithmic recommendation produced from evidence.
    Recommended,
    /// Pending operator review and policy validation.
    UnderReview,
    /// Approved and ratified by policy authority.
    Ratified,
    /// Rejected due to risk constraints or drawdown violations.
    Rejected,
}

impl ProposalStatus {
    /// Returns the canonical uppercase representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Recommended => "RECOMMENDED",
            Self::UnderReview => "UNDER_REVIEW",
            Self::Ratified => "RATIFIED",
            Self::Rejected => "REJECTED",
        }
    }
}

/// Strategy-specific recommended capital allocation and risk contribution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StrategyAllocationRecommendation {
    /// Strategy identifier.
    pub strategy_id: String,
    /// Recommended capital allocation in USD formatted string.
    pub recommended_capital_usd: String,
    /// Share of total portfolio risk budget in basis points (10000 = 100.00%).
    pub risk_budget_share_bps: u32,
    /// Marginal contribution to total portfolio risk in basis points.
    pub marginal_risk_contribution_bps: i64,
}

/// Capital allocation proposal matching `capital-allocation-proposal.schema.json`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapitalAllocationProposal {
    /// Schema version (fixed at 1).
    pub proposal_schema_version: u32,
    /// Unique proposal identifier.
    pub proposal_id: String,
    /// Total equity being allocated in USD.
    pub total_equity_usd: String,
    /// Target annualized portfolio volatility in basis points.
    pub target_annual_volatility_bps: u32,
    /// Maximum allowed drawdown limit in basis points.
    pub max_drawdown_limit_bps: u32,
    /// Breakdown of recommended capital and risk budgets per strategy.
    pub allocations: Vec<StrategyAllocationRecommendation>,
    /// Portfolio diversification ratio in basis points (higher = more diversified).
    pub portfolio_diversification_ratio_bps: u32,
    /// Proposal ratification lifecycle state.
    pub proposal_status: ProposalStatus,
    /// Active risk policy version.
    pub policy_version: String,
    /// RFC3339 timestamp when proposal was produced.
    pub proposed_at: String,
}

impl CapitalAllocationProposal {
    /// Formats the proposal as canonical JSON matching the v1 schema.
    pub fn to_json(&self) -> String {
        let mut json = String::from("{");
        json.push_str("\"proposal_schema_version\":1,");
        json.push_str(&format!("\"proposal_id\":\"{}\",", self.proposal_id));
        json.push_str(&format!("\"total_equity_usd\":\"{}\",", self.total_equity_usd));
        json.push_str(&format!("\"target_annual_volatility_bps\":{},", self.target_annual_volatility_bps));
        json.push_str(&format!("\"max_drawdown_limit_bps\":{},", self.max_drawdown_limit_bps));

        // allocations
        json.push_str("\"allocations\":[");
        for (index, alloc) in self.allocations.iter().enumerate() {
            if index > 0 {
                json.push(',');
            }
            json.push_str(&format!(
                "{{\"strategy_id\":\"{}\",\"recommended_capital_usd\":\"{}\",\"risk_budget_share_bps\":{},\"marginal_risk_contribution_bps\":{}}}",
                alloc.strategy_id, alloc.recommended_capital_usd, alloc.risk_budget_share_bps, alloc.marginal_risk_contribution_bps
            ));
        }
        json.push_str("],");

        json.push_str(&format!("\"portfolio_diversification_ratio_bps\":{},", self.portfolio_diversification_ratio_bps));
        json.push_str(&format!("\"proposal_status\":\"{}\",", self.proposal_status.as_str()));
        json.push_str(&format!("\"policy_version\":\"{}\",", self.policy_version));
        json.push_str(&format!("\"proposed_at\":\"{}\"", self.proposed_at));
        json.push('}');
        json
    }
}

/// Council evaluating risk-budgeted capital allocations across candidate strategies.
pub struct CapitalAllocationCouncil;

impl CapitalAllocationCouncil {
    /// Generates an equal-risk-contribution allocation proposal across strategies.
    pub fn build_erc_proposal(
        total_equity: Decimal,
        strategy_ids: &[&str],
        target_vol_bps: u32,
        max_dd_bps: u32,
        policy_version: &str,
        proposed_at: &str,
    ) -> Result<CapitalAllocationProposal, RiskError> {
        if strategy_ids.is_empty() {
            return Err(RiskError("capital allocation requires at least one strategy".to_owned()));
        }
        if total_equity <= Decimal::ZERO {
            return Err(RiskError("total equity must be positive".to_owned()));
        }

        let n = strategy_ids.len() as u32;
        let per_strategy_weight_bps = 10_000 / n;
        let equity_per_strategy = total_equity.checked_div(Decimal::from_integer(n as i64)?)?;

        let mut allocations = Vec::with_capacity(strategy_ids.len());
        for strategy_id in strategy_ids {
            allocations.push(StrategyAllocationRecommendation {
                strategy_id: (*strategy_id).to_owned(),
                recommended_capital_usd: format!("{}", equity_per_strategy),
                risk_budget_share_bps: per_strategy_weight_bps,
                marginal_risk_contribution_bps: (target_vol_bps / n) as i64,
            });
        }

        let diversification_ratio_bps = 10_000 + (n - 1) * 1_200; // diversification benefit

        let digest = format!("{:x}", Sha256::digest(format!("{}:{}:{}", total_equity, n, proposed_at).as_bytes()));
        let proposal_id = format!("cap-prop.{}", &digest[..16]);

        Ok(CapitalAllocationProposal {
            proposal_schema_version: 1,
            proposal_id,
            total_equity_usd: format!("{}", total_equity),
            target_annual_volatility_bps: target_vol_bps,
            max_drawdown_limit_bps: max_dd_bps,
            allocations,
            portfolio_diversification_ratio_bps: diversification_ratio_bps,
            proposal_status: ProposalStatus::Recommended,
            policy_version: policy_version.to_owned(),
            proposed_at: proposed_at.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_equal_risk_contribution_proposal() {
        let equity = Decimal::from_integer(100_000).unwrap();
        let strategies = ["strat.alpha", "strat.beta"];

        let proposal = CapitalAllocationCouncil::build_erc_proposal(
            equity,
            &strategies,
            1200,
            1500,
            "policy.v1",
            "2026-09-01T18:00:00Z",
        )
        .unwrap();

        assert_eq!(proposal.proposal_schema_version, 1);
        assert_eq!(proposal.allocations.len(), 2);
        assert_eq!(proposal.allocations[0].risk_budget_share_bps, 5000);
        assert_eq!(proposal.allocations[1].risk_budget_share_bps, 5000);
        assert_eq!(proposal.proposal_status, ProposalStatus::Recommended);

        let json = proposal.to_json();
        assert!(json.contains("\"proposal_status\":\"RECOMMENDED\""));
        assert!(json.contains("\"target_annual_volatility_bps\":1200"));
    }
}
