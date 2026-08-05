# Domain glossary

| Term | Meaning |
| --- | --- |
| Instrument | Canonical tradable entity identified by an internal `instrument_id`, not a ticker |
| Market event | Normalized quote, trade, bar, session, or corporate-action event with source and receive times |
| Strategy | User-defined logic running outside the risk-critical core |
| Order intent | A request from a strategy or user to trade; it is not a broker order |
| Risk decision | Versioned approval or rejection of an order intent, including reasons and applied limits |
| Order | The OMS-managed executable instruction and its lifecycle state |
| Execution/fill | Broker-confirmed quantity and price that changes order and portfolio state |
| Position | Internal record of quantity, cost, P&L, and attribution for an instrument/account |
| Replay | Deterministic reprocessing of recorded events, configuration, and software version |
| Reconciliation | Comparison of internal orders, executions, cash, and positions with broker records |
| Kill switch | A control that blocks new trading independently of the strategy process |
| Shadow mode | Live-data execution simulation that records intents but sends no broker order |

## Ubiquitous language rules

- Never call an unapproved request an “order”; call it an **order intent**.
- Never infer an order failure after a disconnect; use the **UNKNOWN** state until reconciliation proves otherwise.
- “Live” means a connected broker account capable of transmitting real orders. “Paper” and “simulation” are distinct non-live environments.
