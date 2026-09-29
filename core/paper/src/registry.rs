//! Account-isolated registry of PAPER broker routes.

use follon_domain::validate_canonical_id;
use std::collections::{BTreeMap, BTreeSet};

use crate::*;

/// Deterministic composition root for isolated PAPER broker-account routes.
///
/// The registry is only an OMS-side adapter selection mechanism. A route has
/// no credentials and only delegates normalized requests after the caller's
/// existing risk/OMS processing. `BTreeMap` storage makes route enumeration
/// stable and unknown or duplicate bindings fail closed.
pub struct PaperBrokerRegistry {
    routes: BTreeMap<String, PaperBrokerRoute>,
    adapters: BTreeMap<String, Box<dyn PaperBrokerAdapter>>,
    adapter_configuration_fingerprints: BTreeMap<String, String>,
    legacy_fingerprint_accounts: BTreeSet<String>,
}

impl Default for PaperBrokerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PaperBrokerRegistry {
    /// Creates an empty registry; a deployment must register every PAPER route explicitly.
    pub fn new() -> Self {
        Self {
            routes: BTreeMap::new(),
            adapters: BTreeMap::new(),
            adapter_configuration_fingerprints: BTreeMap::new(),
            legacy_fingerprint_accounts: BTreeSet::new(),
        }
    }

    /// Registers one isolated adapter instance for one canonical PAPER account.
    pub fn register(
        &mut self,
        route: PaperBrokerRoute,
        adapter: Box<dyn PaperBrokerAdapter>,
    ) -> Result<(), PaperError> {
        self.register_inner(route, adapter, false)
    }

    /// Registers the exact legacy local-IBKR PAPER composition without changing
    /// its journal fingerprint.
    ///
    /// This is only a migration bridge for configuration schema v1 and can
    /// reopen, but not initialize, a journal. New configurations must use
    /// [`Self::register`] so the durable journal binds the route and actual
    /// adapter configuration fingerprints.
    pub fn register_legacy_ibkr_paper_route(
        &mut self,
        account: &PaperAccount,
    ) -> Result<(), PaperError> {
        account.validate()?;
        let route = PaperBrokerRoute {
            account_id: account.account_id.clone(),
            adapter_id: format!("adapter.ibkr.paper.{}", account.account_id),
            venue_id: "venue.ibkr.paper".to_owned(),
            environment: "PAPER".to_owned(),
        };
        self.register_inner(route, Box::new(IbkrPaperAdapter::new(account)?), true)
    }

    fn register_inner(
        &mut self,
        route: PaperBrokerRoute,
        adapter: Box<dyn PaperBrokerAdapter>,
        preserve_legacy_fingerprint: bool,
    ) -> Result<(), PaperError> {
        route.validate()?;
        if self.routes.contains_key(&route.account_id) {
            return Err(PaperError(
                "paper broker route already exists for account".to_owned(),
            ));
        }
        if self.adapters.contains_key(&route.adapter_id) {
            return Err(PaperError(
                "paper broker adapter_id is already registered".to_owned(),
            ));
        }
        let adapter_configuration_fingerprint =
            adapter.adapter_configuration_fingerprint(&route.account_id)?;
        if adapter_configuration_fingerprint.is_empty() {
            return Err(PaperError(
                "paper broker adapter must expose a non-secret configuration fingerprint"
                    .to_owned(),
            ));
        }
        self.adapters.insert(route.adapter_id.clone(), adapter);
        self.adapter_configuration_fingerprints
            .insert(route.adapter_id.clone(), adapter_configuration_fingerprint);
        if preserve_legacy_fingerprint {
            self.legacy_fingerprint_accounts
                .insert(route.account_id.clone());
        }
        self.routes.insert(route.account_id.clone(), route);
        Ok(())
    }

    /// Returns the configured routes in stable account-id order.
    pub fn routes(&self) -> Vec<PaperBrokerRoute> {
        self.routes.values().cloned().collect()
    }

    fn adapter_for_account(
        &mut self,
        account_id: &str,
    ) -> Result<&mut (dyn PaperBrokerAdapter + '_), PaperError> {
        validate_canonical_id("paper broker route account_id", account_id)?;
        let adapter_id = self
            .routes
            .get(account_id)
            .ok_or_else(|| {
                PaperError("paper broker route is not configured for account".to_owned())
            })?
            .adapter_id
            .clone();
        match self.adapters.get_mut(&adapter_id) {
            Some(adapter) => Ok(adapter.as_mut()),
            None => Err(PaperError(
                "paper broker route adapter is unavailable".to_owned(),
            )),
        }
    }
}

impl PaperBrokerAdapter for PaperBrokerRegistry {
    /// A route carries exactly what its adapter declares.
    fn capabilities(&self, account_id: &str) -> Result<PaperBrokerCapabilities, PaperError> {
        validate_canonical_id("paper broker route account_id", account_id)?;
        let route = self.routes.get(account_id).ok_or_else(|| {
            PaperError("paper broker route is not configured for account".to_owned())
        })?;
        self.adapters
            .get(&route.adapter_id)
            .ok_or_else(|| PaperError("paper broker route adapter is unavailable".to_owned()))?
            .capabilities(account_id)
    }

    fn configuration_fingerprint(&self, account_id: &str) -> Result<String, PaperError> {
        validate_canonical_id("paper broker route account_id", account_id)?;
        let route = self.routes.get(account_id).ok_or_else(|| {
            PaperError("paper broker route is not configured for account".to_owned())
        })?;
        if self.legacy_fingerprint_accounts.contains(account_id) {
            return Ok(String::new());
        }
        let adapter_configuration_fingerprint = self
            .adapter_configuration_fingerprints
            .get(&route.adapter_id)
            .ok_or_else(|| PaperError("paper broker route adapter is unavailable".to_owned()))?;
        Ok(hash_fingerprint_parts(&[
            "paper-broker-route-v2",
            &route.account_id,
            &route.adapter_id,
            &route.venue_id,
            &route.environment,
            adapter_configuration_fingerprint,
        ]))
    }

    fn permits_empty_journal(&self, account_id: &str) -> bool {
        !self.legacy_fingerprint_accounts.contains(account_id)
    }

    fn submit(&mut self, request: &BrokerOrderRequest) -> Result<BrokerSubmitResult, PaperError> {
        request.validate()?;
        self.adapter_for_account(&request.account_id)?
            .submit(request)
    }

    fn submit_combo(
        &mut self,
        request: &BrokerComboRequest,
    ) -> Result<BrokerSubmitResult, PaperError> {
        request.validate()?;
        self.adapter_for_account(&request.account_id)?
            .submit_combo(request)
    }

    fn cancel(&mut self, request: &BrokerCancelRequest) -> Result<(), PaperError> {
        request.validate()?;
        self.adapter_for_account(&request.account_id)?
            .cancel(request)
    }

    fn replace(&mut self, request: &BrokerReplaceRequest) -> Result<(), PaperError> {
        request.validate()?;
        self.adapter_for_account(&request.account_id)?
            .replace(request)
    }

    fn poll(&mut self, account_id: &str) -> Result<Vec<BrokerEvent>, PaperError> {
        self.adapter_for_account(account_id)?.poll(account_id)
    }

    fn snapshot(&mut self, account_id: &str) -> Result<BrokerAccountSnapshot, PaperError> {
        self.adapter_for_account(account_id)?.snapshot(account_id)
    }

    fn reconnect(&mut self, account_id: &str) -> Result<(), PaperError> {
        self.adapter_for_account(account_id)?.reconnect(account_id)
    }
}
