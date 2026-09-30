//! Portfolio-wide deterministic risk aggregation and pre-trade controls.
//!
//! The evaluator combines every account/strategy/asset/currency exposure before
//! deciding. It produces evidence only; OMS remains the sole order authority.

pub mod allocation;
mod error;
mod evaluate;
mod numeric;
mod policy;
mod position;

pub use allocation::{
    CapitalAllocationCouncil, CapitalAllocationProposal, ProposalStatus,
    StrategyAllocationRecommendation,
};
pub use error::*;
pub use evaluate::*;
use numeric::*;
pub use policy::*;
pub use position::*;

#[cfg(test)]
mod tests;
