# Adapters

This directory contains broker integration boundaries and will also hold
market-data and notification integrations. Adapters translate external formats
to canonical contracts and must not own risk policy, portfolio truth, or
domain-state transitions.

PAPER deployment composition binds every canonical account to one explicit
`PaperBrokerRoute` in the core-owned `PaperBrokerRegistry`. The registry routes
only OMS-originated normalized requests; it holds no credentials and is not
available to strategies, SDKs, desktop clients, or mobile clients. Cancel,
replace, poll, snapshot, and reconnect operations are all account scoped, so a
client order identity from one account cannot be sent to another adapter route.
Routes are stored in deterministic account order and unknown or duplicate
bindings fail closed. New durable routes fingerprint both this metadata and the
adapter's non-secret implementation/configuration evidence, so recovery rejects
an unnoticed transport or venue change.

The current IBKR adapter remains PAPER-only: the Rust transport uses a bounded
private process protocol and `python/ibkr-gateway` hosts the official TWS API
client. One configured IBKR PAPER account maps to one registry route and
adapter instance; additional routes require separately reviewed adapter and
reconciliation evidence. No new live adapter is enabled by this framework.
