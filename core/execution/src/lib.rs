//! Deterministic execution-management algorithms.
//!
//! This crate converts an already risk-approved parent order into auditable
//! child instructions. It never contacts a broker and cannot bypass OMS/risk.

mod algorithms;
mod basket;
mod combo;
mod error;
mod evidence;
mod passive;
mod plan;
mod protective;
mod routing;
mod tca;

pub use algorithms::*;
pub use basket::*;
pub use combo::*;
pub use error::*;
pub use evidence::*;
pub use passive::*;
pub use plan::*;
pub use protective::*;
pub use routing::*;
pub use tca::*;

#[cfg(test)]
mod tests;
