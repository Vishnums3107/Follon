//! Operator and local kill-switch control.

use follon_domain::{validate_canonical_id, validate_utc_timestamp};

use crate::*;

impl<B: PaperBrokerAdapter> PaperTradingService<B> {
    /// Activates a kill switch without involving strategy or broker processes.
    pub fn activate_kill_switch(&mut self, scope: KillSwitchScope) -> Result<bool, PaperError> {
        self.ensure_persistence_healthy()?;
        let changed = self.kill_switches.activate(scope)?;
        self.persist()?;
        Ok(changed)
    }

    /// Explicitly deactivates one kill switch. It does not mutate past decisions.
    pub fn deactivate_kill_switch(&mut self, scope: &KillSwitchScope) -> Result<bool, PaperError> {
        self.ensure_persistence_healthy()?;
        let changed = self.kill_switches.deactivate(scope);
        if changed {
            self.persist()?;
        }
        Ok(changed)
    }

    /// Activates a kill switch for an authenticated operator (E3.3b). The
    /// change is journaled with the operator and time. Activating a switch
    /// that is already active changes nothing and journals nothing.
    pub fn activate_kill_switch_as(
        &mut self,
        scope: KillSwitchScope,
        operator: &str,
        operated_at: &str,
    ) -> Result<bool, PaperError> {
        self.operate_kill_switch_as(scope, KillSwitchAction::Activate, operator, operated_at)
    }

    /// Releases a kill switch for an authenticated operator, journaled exactly
    /// as [`Self::activate_kill_switch_as`] journals an activation.
    pub fn release_kill_switch_as(
        &mut self,
        scope: KillSwitchScope,
        operator: &str,
        operated_at: &str,
    ) -> Result<bool, PaperError> {
        self.operate_kill_switch_as(scope, KillSwitchAction::Release, operator, operated_at)
    }

    /// Operator-attributed kill-switch changes, oldest first.
    pub fn kill_switch_operations(&self) -> &[KillSwitchOperation] {
        &self.kill_switch_operations
    }

    /// Operator-attributed order commands, oldest first (E5.2b).
    pub fn order_operations(&self) -> &[OrderOperation] {
        &self.order_operations
    }

    fn operate_kill_switch_as(
        &mut self,
        scope: KillSwitchScope,
        action: KillSwitchAction,
        operator: &str,
        operated_at: &str,
    ) -> Result<bool, PaperError> {
        self.ensure_persistence_healthy()?;
        scope.validate()?;
        validate_canonical_id("kill-switch operator", operator)?;
        validate_utc_timestamp("kill-switch operation time", operated_at)?;
        let changed = match action {
            KillSwitchAction::Activate => self.kill_switches.activate(scope.clone())?,
            KillSwitchAction::Release => self.kill_switches.deactivate(&scope),
        };
        if changed {
            self.kill_switch_operations.push(KillSwitchOperation {
                scope: scope.as_key(),
                action,
                operator: operator.to_owned(),
                operated_at: operated_at.to_owned(),
            });
            self.persist()?;
        }
        Ok(changed)
    }
}
