# Adapters

This directory contains broker integration boundaries and will also hold
market-data and notification integrations. Adapters translate external formats
to canonical contracts and must not own risk policy, portfolio truth, or
domain-state transitions. The current IBKR adapter is PAPER-only: the Rust
transport uses a bounded private process protocol and `python/ibkr-gateway`
hosts the official TWS API client. No live adapter is present.
