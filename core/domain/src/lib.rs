//! Stable, framework-independent contracts for the Follon trading kernel.
//!
//! Values that affect accounting are represented as fixed-point [`Decimal`]s;
//! broker and UI concerns intentionally do not appear in this crate.

pub mod compatibility;

mod combo;
mod decimal;
mod error;
mod events;
mod lifecycle;
mod market;
mod order;

pub use compatibility::{
    CompatibilityMatrix, CompatibilityRegistry, SchemaCompatibilityEntry, SchemaMigrationStatus,
};

pub use combo::*;
pub use decimal::*;
pub use error::*;
pub use events::*;
pub use lifecycle::*;
pub use market::*;
pub use order::*;

#[cfg(test)]
mod tests;
