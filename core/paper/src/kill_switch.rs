//! Kill-switch scopes and registry, and the operator-attributed operation records.

use follon_domain::{validate_canonical_id, ComboIntent, OrderIntent};
use std::collections::BTreeSet;

use crate::*;

/// Scope at which a kill switch independently blocks new paper orders.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum KillSwitchScope {
    /// Blocks all paper accounts and strategies.
    Global,
    /// Blocks one account.
    Account(String),
    /// Blocks one strategy.
    Strategy(String),
    /// Blocks one instrument.
    Instrument(String),
}

impl KillSwitchScope {
    /// Stable display and dashboard identity for this operational control.
    pub fn as_key(&self) -> String {
        match self {
            Self::Global => "global".to_owned(),
            Self::Account(account_id) => format!("account:{account_id}"),
            Self::Strategy(strategy_id) => format!("strategy:{strategy_id}"),
            Self::Instrument(instrument_id) => format!("instrument:{instrument_id}"),
        }
    }

    pub(crate) fn validate(&self) -> Result<(), PaperError> {
        match self {
            Self::Global => Ok(()),
            Self::Account(value) => {
                validate_canonical_id("kill-switch account", value).map_err(Into::into)
            }
            Self::Strategy(value) => {
                validate_canonical_id("kill-switch strategy", value).map_err(Into::into)
            }
            Self::Instrument(value) => {
                validate_canonical_id("kill-switch instrument", value).map_err(Into::into)
            }
        }
    }

    /// Parses a scope from its stable key, the inverse of [`Self::as_key`]:
    /// `global`, `account:<id>`, `strategy:<id>` or `instrument:<id>`, with a
    /// canonical identity.
    pub fn from_key(value: &str) -> Result<Self, PaperError> {
        if value == "global" {
            return Ok(Self::Global);
        }
        for (prefix, builder) in [
            ("account:", Self::Account as fn(String) -> Self),
            ("strategy:", Self::Strategy as fn(String) -> Self),
            ("instrument:", Self::Instrument as fn(String) -> Self),
        ] {
            if let Some(target) = value.strip_prefix(prefix) {
                let scope = builder(target.to_owned());
                scope.validate()?;
                return Ok(scope);
            }
        }
        Err(PaperError(
            "kill-switch scope must be global, account:<id>, strategy:<id> or instrument:<id>"
                .to_owned(),
        ))
    }
}

/// Which way an operator moved a kill switch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KillSwitchAction {
    /// The switch was activated.
    Activate,
    /// The switch was released.
    Release,
}

impl KillSwitchAction {
    /// Stable journal identity.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Activate => "ACTIVATE",
            Self::Release => "RELEASE",
        }
    }
}

/// One operator-attributed kill-switch change, as the PAPER journal records
/// it (E3.3b).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KillSwitchOperation {
    /// The scope's stable key ([`KillSwitchScope::as_key`]).
    pub scope: String,
    /// Whether the switch was activated or released.
    pub action: KillSwitchAction,
    /// The authenticated operator who made the change.
    pub operator: String,
    /// Canonical UTC time the change was applied.
    pub operated_at: String,
}

/// What an operator asked of an order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OrderOperationAction {
    /// The operator requested cancellation, and the order moved to
    /// `PENDING_CANCEL`.
    CancelRequested,
}

impl OrderOperationAction {
    /// Stable journal identity.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CancelRequested => "CANCEL_REQUESTED",
        }
    }
}

/// One operator-attributed order command, as the PAPER journal records it
/// (delivery state E5.2b).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrderOperation {
    /// The single or combination order the command addressed.
    pub order_id: String,
    /// What the operator asked.
    pub action: OrderOperationAction,
    /// The authenticated operator who asked.
    pub operator: String,
    /// Canonical UTC time the command was applied.
    pub operated_at: String,
}

/// Versioned independently-operable paper kill-switch registry.
#[derive(Clone, Debug)]
pub struct KillSwitchRegistry {
    /// Immutable registry/policy revision used in operations evidence.
    pub version: String,
    active: BTreeSet<KillSwitchScope>,
}

impl KillSwitchRegistry {
    /// Creates an initially clear registry with an immutable version identity.
    pub fn new(version: impl Into<String>) -> Result<Self, PaperError> {
        let registry = Self {
            version: version.into(),
            active: BTreeSet::new(),
        };
        if registry.version.is_empty() {
            return Err(PaperError("kill-switch version is required".to_owned()));
        }
        Ok(registry)
    }

    /// Activates a kill switch independently of strategy or broker health.
    pub fn activate(&mut self, scope: KillSwitchScope) -> Result<bool, PaperError> {
        scope.validate()?;
        Ok(self.active.insert(scope))
    }

    /// Deactivates a kill switch explicitly; it never changes historical evidence.
    pub fn deactivate(&mut self, scope: &KillSwitchScope) -> bool {
        self.active.remove(scope)
    }

    /// Lists active scopes in deterministic operational-display order.
    pub fn active_keys(&self) -> Vec<String> {
        self.active.iter().map(KillSwitchScope::as_key).collect()
    }

    pub(crate) fn rejection_reasons(&self, intent: &OrderIntent) -> Vec<String> {
        self.reasons_for(
            &intent.account_id,
            &intent.strategy_id,
            std::slice::from_ref(&intent.instrument_id),
        )
    }

    /// Kill-switch reasons for a combination.
    ///
    /// An instrument switch on *any* leg blocks the whole combination. There is
    /// no partial execution to fall back on — the group is atomic — so a single
    /// halted leg halts the structure.
    pub(crate) fn combo_rejection_reasons(&self, intent: &ComboIntent) -> Vec<String> {
        let instruments = intent
            .legs
            .iter()
            .map(|leg| leg.instrument_id.clone())
            .collect::<Vec<_>>();
        self.reasons_for(&intent.account_id, &intent.strategy_id, &instruments)
    }

    fn reasons_for(
        &self,
        account_id: &str,
        strategy_id: &str,
        instrument_ids: &[String],
    ) -> Vec<String> {
        let mut scopes = vec![
            KillSwitchScope::Global,
            KillSwitchScope::Account(account_id.to_owned()),
            KillSwitchScope::Strategy(strategy_id.to_owned()),
        ];
        scopes.extend(
            instrument_ids
                .iter()
                .map(|instrument_id| KillSwitchScope::Instrument(instrument_id.clone())),
        );
        let mut reasons = scopes
            .iter()
            .filter(|scope| self.active.contains(*scope))
            .map(|scope| {
                format!(
                    "KILL_SWITCH_{}",
                    scope.as_key().to_ascii_uppercase().replace(':', "_")
                )
            })
            .collect::<Vec<_>>();
        reasons.dedup();
        reasons
    }
}
