//! Broker fault injection for exercising UNKNOWN-state handling.

use std::collections::{BTreeMap, VecDeque};

use crate::*;

/// One broker operation that can receive an injected reliability fault.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum BrokerOperation {
    /// Submission of a new order.
    Submit,
    /// Cancellation of an existing order.
    Cancel,
    /// Replacement of a working limit order.
    Replace,
    /// Polling asynchronous broker evidence.
    Poll,
    /// Reconnection attempt.
    Reconnect,
}

/// Deterministic fault modes exercised before real paper promotion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BrokerFault {
    /// No broker call is made; the operation is disconnected before a known outcome.
    Disconnect,
    /// The inner broker accepts a submit, but the caller receives an ambiguous failure.
    AmbiguousAfterSubmit,
    /// Repeats the first broker event returned by a poll operation.
    DuplicateFirstEvent,
}

/// Fault-injection wrapper for any paper broker adapter.
pub struct FaultInjectingBroker<B> {
    pub(crate) inner: B,
    faults: BTreeMap<BrokerOperation, VecDeque<BrokerFault>>,
}

impl<B> FaultInjectingBroker<B> {
    /// Wraps a concrete paper adapter without changing its normal behavior.
    pub fn new(inner: B) -> Self {
        Self {
            inner,
            faults: BTreeMap::new(),
        }
    }

    /// Schedules one deterministic fault for a future operation.
    pub fn inject(&mut self, operation: BrokerOperation, fault: BrokerFault) {
        self.faults.entry(operation).or_default().push_back(fault);
    }

    /// Returns the underlying adapter after a test scenario completes.
    pub fn into_inner(self) -> B {
        self.inner
    }

    fn next_fault(&mut self, operation: BrokerOperation) -> Option<BrokerFault> {
        self.faults
            .get_mut(&operation)
            .and_then(VecDeque::pop_front)
    }
}

impl<B: PaperBrokerAdapter> PaperBrokerAdapter for FaultInjectingBroker<B> {
    /// Exactly the inner adapter's set: every operation it declares is
    /// forwarded, under this wrapper's fault schedule.
    fn capabilities(&self, account_id: &str) -> Result<PaperBrokerCapabilities, PaperError> {
        self.inner.capabilities(account_id)
    }

    fn adapter_configuration_fingerprint(&self, account_id: &str) -> Result<String, PaperError> {
        self.inner.adapter_configuration_fingerprint(account_id)
    }

    fn configuration_fingerprint(&self, account_id: &str) -> Result<String, PaperError> {
        self.inner.configuration_fingerprint(account_id)
    }

    fn permits_empty_journal(&self, account_id: &str) -> bool {
        self.inner.permits_empty_journal(account_id)
    }

    fn submit(&mut self, request: &BrokerOrderRequest) -> Result<BrokerSubmitResult, PaperError> {
        match self.next_fault(BrokerOperation::Submit) {
            Some(BrokerFault::Disconnect) => Err(PaperError(
                "fault injection disconnected before paper submission".to_owned(),
            )),
            Some(BrokerFault::AmbiguousAfterSubmit) => {
                let _ = self.inner.submit(request)?;
                Err(PaperError(
                    "fault injection made paper submission outcome ambiguous".to_owned(),
                ))
            }
            Some(BrokerFault::DuplicateFirstEvent) | None => self.inner.submit(request),
        }
    }

    /// Combinations follow the same submission fault schedule as single
    /// orders. Before E5.1 this wrapper did not forward them, so every
    /// combination met the trait's refusal, which the OMS recorded as
    /// `UNKNOWN` whether or not a fault was scheduled.
    fn submit_combo(
        &mut self,
        request: &BrokerComboRequest,
    ) -> Result<BrokerSubmitResult, PaperError> {
        match self.next_fault(BrokerOperation::Submit) {
            Some(BrokerFault::Disconnect) => Err(PaperError(
                "fault injection disconnected before paper combination submission".to_owned(),
            )),
            Some(BrokerFault::AmbiguousAfterSubmit) => {
                let _ = self.inner.submit_combo(request)?;
                Err(PaperError(
                    "fault injection made paper combination submission outcome ambiguous"
                        .to_owned(),
                ))
            }
            Some(BrokerFault::DuplicateFirstEvent) | None => self.inner.submit_combo(request),
        }
    }

    fn cancel(&mut self, request: &BrokerCancelRequest) -> Result<(), PaperError> {
        match self.next_fault(BrokerOperation::Cancel) {
            Some(BrokerFault::Disconnect) | Some(BrokerFault::AmbiguousAfterSubmit) => Err(
                PaperError("fault injection made paper cancellation outcome ambiguous".to_owned()),
            ),
            Some(BrokerFault::DuplicateFirstEvent) | None => self.inner.cancel(request),
        }
    }

    fn replace(&mut self, request: &BrokerReplaceRequest) -> Result<(), PaperError> {
        match self.next_fault(BrokerOperation::Replace) {
            Some(BrokerFault::Disconnect) | Some(BrokerFault::AmbiguousAfterSubmit) => Err(
                PaperError("fault injection made paper replacement outcome ambiguous".to_owned()),
            ),
            Some(BrokerFault::DuplicateFirstEvent) | None => self.inner.replace(request),
        }
    }

    fn poll(&mut self, account_id: &str) -> Result<Vec<BrokerEvent>, PaperError> {
        match self.next_fault(BrokerOperation::Poll) {
            Some(BrokerFault::Disconnect) | Some(BrokerFault::AmbiguousAfterSubmit) => Err(
                PaperError("fault injection disconnected paper polling".to_owned()),
            ),
            Some(BrokerFault::DuplicateFirstEvent) => {
                let mut events = self.inner.poll(account_id)?;
                if let Some(first) = events.first().cloned() {
                    events.push(first);
                }
                Ok(events)
            }
            None => self.inner.poll(account_id),
        }
    }

    fn snapshot(&mut self, account_id: &str) -> Result<BrokerAccountSnapshot, PaperError> {
        self.inner.snapshot(account_id)
    }

    fn reconnect(&mut self, account_id: &str) -> Result<(), PaperError> {
        match self.next_fault(BrokerOperation::Reconnect) {
            Some(_) => Err(PaperError(
                "fault injection rejected paper reconnect".to_owned(),
            )),
            None => self.inner.reconnect(account_id),
        }
    }
}
