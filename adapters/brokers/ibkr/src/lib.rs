//! Interactive Brokers paper-gateway configuration and normalized adapter contract.
//!
//! The adapter deliberately permits only documented paper ports. It translates
//! no strategy or risk decision: the core submits normalized requests only after
//! its own OMS and risk controls accept them. A TWS/Gateway transport plugs into
//! [`IbkrPaperGatewayTransport`] and is continuously exercised by the core's
//! deterministic in-memory paper model and fault-injection suite.

use follon_domain::validate_canonical_id;
use follon_paper::{
    BrokerAccountSnapshot, BrokerEvent, BrokerOrderRequest, BrokerSubmitResult, PaperBrokerAdapter,
    PaperError,
};

/// IBKR TWS/Gateway endpoint allowed for paper operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IbkrPaperGatewayConfiguration {
    /// Canonical account identity selected for the paper session.
    pub account_id: String,
    /// Local TWS/Gateway hostname; public and live endpoints are refused.
    pub host: String,
    /// IBKR paper TWS (7497) or Gateway (4002) port.
    pub port: u16,
    /// Must be the literal value `PAPER`.
    pub environment: String,
}

impl IbkrPaperGatewayConfiguration {
    /// Rejects any endpoint outside an explicit local IBKR paper gateway.
    pub fn validate(&self) -> Result<(), PaperError> {
        validate_canonical_id("IBKR paper account_id", &self.account_id)?;
        if !matches!(self.host.as_str(), "127.0.0.1" | "localhost" | "::1")
            || !matches!(self.port, 7497 | 4002)
            || self.environment != "PAPER"
        {
            return Err(PaperError(
                "IBKR adapter accepts only a local PAPER TWS (7497) or Gateway (4002) endpoint"
                    .to_owned(),
            ));
        }
        Ok(())
    }
}

/// Minimal transport implemented by a pinned, audited IBKR TWS/Gateway client.
///
/// This deliberate seam keeps vendor wire parsing outside the OMS. A transport
/// must preserve the pre-generated `client_order_id` as the adapter idempotency
/// key and return normalized evidence; it has no permission to create orders,
/// bypass a kill switch, or alter risk decisions.
pub trait IbkrPaperGatewayTransport {
    /// Sends one normalized paper order request.
    fn submit_paper_order(
        &mut self,
        request: &BrokerOrderRequest,
    ) -> Result<BrokerSubmitResult, PaperError>;
    /// Requests cancellation of one pre-generated client order identity.
    fn cancel_paper_order(&mut self, client_order_id: &str) -> Result<(), PaperError>;
    /// Drains normalized IBKR order/execution evidence.
    fn poll_paper_events(&mut self) -> Result<Vec<BrokerEvent>, PaperError>;
    /// Retrieves an independent IBKR paper account snapshot.
    fn paper_account_snapshot(
        &mut self,
        account_id: &str,
    ) -> Result<BrokerAccountSnapshot, PaperError>;
    /// Reconnects to the configured paper endpoint; reconciliation follows in the core.
    fn reconnect_paper(&mut self) -> Result<(), PaperError>;
}

/// Concrete normalized adapter around one pinned IBKR TWS/Gateway paper transport.
pub struct IbkrPaperGatewayAdapter<T> {
    configuration: IbkrPaperGatewayConfiguration,
    transport: T,
}

impl<T: IbkrPaperGatewayTransport> IbkrPaperGatewayAdapter<T> {
    /// Creates an adapter only after the endpoint is proved paper-only and local.
    pub fn new(
        configuration: IbkrPaperGatewayConfiguration,
        transport: T,
    ) -> Result<Self, PaperError> {
        configuration.validate()?;
        Ok(Self {
            configuration,
            transport,
        })
    }

    /// Returns the immutable endpoint configuration for health/status projection.
    pub fn configuration(&self) -> &IbkrPaperGatewayConfiguration {
        &self.configuration
    }

    /// Releases the configured vendor transport during controlled shutdown.
    pub fn into_transport(self) -> T {
        self.transport
    }
}

impl<T: IbkrPaperGatewayTransport> PaperBrokerAdapter for IbkrPaperGatewayAdapter<T> {
    fn submit(&mut self, request: &BrokerOrderRequest) -> Result<BrokerSubmitResult, PaperError> {
        if request.account_id != self.configuration.account_id {
            return Err(PaperError(
                "IBKR paper request account does not match configuration".to_owned(),
            ));
        }
        self.transport.submit_paper_order(request)
    }

    fn cancel(&mut self, client_order_id: &str) -> Result<(), PaperError> {
        self.transport.cancel_paper_order(client_order_id)
    }

    fn poll(&mut self) -> Result<Vec<BrokerEvent>, PaperError> {
        self.transport.poll_paper_events()
    }

    fn snapshot(&mut self, account_id: &str) -> Result<BrokerAccountSnapshot, PaperError> {
        if account_id != self.configuration.account_id {
            return Err(PaperError(
                "IBKR paper snapshot account does not match configuration".to_owned(),
            ));
        }
        self.transport.paper_account_snapshot(account_id)
    }

    fn reconnect(&mut self) -> Result<(), PaperError> {
        self.transport.reconnect_paper()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_configuration_refuses_live_and_nonlocal_endpoints() {
        let configuration = IbkrPaperGatewayConfiguration {
            account_id: "acct.paper.001".to_owned(),
            host: "127.0.0.1".to_owned(),
            port: 7497,
            environment: "PAPER".to_owned(),
        };
        assert!(configuration.validate().is_ok());
        assert!(IbkrPaperGatewayConfiguration {
            environment: "LIVE".to_owned(),
            ..configuration.clone()
        }
        .validate()
        .is_err());
        assert!(IbkrPaperGatewayConfiguration {
            host: "api.ibkr.example".to_owned(),
            ..configuration
        }
        .validate()
        .is_err());
    }
}
