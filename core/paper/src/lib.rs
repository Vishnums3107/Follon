//! Paper-only operational OMS, risk controls, broker fault injection, and reconciliation.
//!
//! This crate is intentionally incapable of live trading. It owns the state
//! between a validated paper intent and a normalized broker response, preserving
//! the safe `UNKNOWN` lifecycle state whenever submission or cancellation cannot
//! be proven. Broker snapshots are compared against independent internal state;
//! reconciliation never silently overwrites that state.

mod account;
mod broker;
mod combinations;
mod dashboard;
mod error;
mod fault;
mod fingerprint;
mod ibkr;
mod journal;
mod kill_switch;
mod market;
mod order;
mod policy;
mod portfolio_risk_document;
pub mod qualification;
mod reconciliation;
mod records;
mod registry;
mod service;
mod validation;

use combinations::PersistentComboExecutionState;
pub use combinations::{BrokerComboExecution, BrokerComboExecutionLeg};

pub use qualification::{
    GatewayQualificationError, GatewayQualificationMatrix, QualificationState, QualifiedCapability,
};

pub use account::*;
pub use broker::*;
pub use dashboard::*;
pub use error::*;
pub use fault::*;
use fingerprint::*;
pub use ibkr::*;
pub use journal::*;
pub use kill_switch::*;
pub use market::*;
pub use order::*;
pub use policy::*;
pub use portfolio_risk_document::*;
pub use reconciliation::*;
use records::*;
pub use registry::*;
pub use service::*;
use validation::*;

#[cfg(test)]
mod tests;
