# Follon — Solo Trading Operating System

Follon is a risk-first, multi-asset trading operating system designed for advanced independent traders and small professional teams. Its defining principle is **research-to-live parity**, ensuring a strategy behaves equivalently across research, deterministic replay, simulation, paper trading, and controlled live execution.

## Features and Functionalities

### Core Trading & Risk Kernel
- **Research-to-Live Parity:** Deterministic backtest runner, identical execution paths, append-only local NDJSON event storage, and cumulative portfolio evidence flow.
- **Risk/OMS Gateway:** A PAPER-only OMS with versioned risk limits, fresh-market checks, cash reservation, reconciliation, and kill switches. The OMS is the sole authority allowed to submit orders through a broker adapter.
- **Broker-Neutral EMS Algorithms:** Support for immediate, TWAP, VWAP, participation, arrival-price, passive cancel/replace, routing, brackets, trailing stops, baskets, and atomic option combinations.
- **Deterministic European-Options Core:** Features versioned chain snapshots, fixed-point implied volatility/Greeks, multi-leg expiry scenarios, and explicit cash/physical exercise settlement.

### Desktop Trading Terminal
- **Architecture:** Built with React/TypeScript and packaged with Vite and Tauri v2 for a privileged native boundary.
- **Capabilities:** Active PAPER and LIVE order-entry controls, cancel, and position-close controls alongside monitoring, operations, portfolio, identity, platform, and acceptance-gate views.
- **IPC Boundaries:** The desktop uses privileged IPC commands to submit declarative order intents to the Risk/OMS route, meaning it never contacts a broker directly.

### Integrations and Services
- **Python SDKs:** Python strategy contracts that can submit intents but are architecturally barred from accessing adapters or credentials directly.
- **gRPC API:** Scheduled, passive, combination, risk, and margin planning are exposed through a versioned gRPC API.
- **PostgreSQL Persistence:** Transactional migrations and adapter behavior for tenant-isolated events/outbox, balanced journals, IAM, and broker command receipts.

### Operations and Security
- **Identity & Access Management (IAM):** Customer IAM primitives featuring Argon2id, TOTP MFA, lockout, short opaque sessions, revocation, tenant isolation, and explicit RBAC permissions.
- **Operator Workbench:** A deterministic operator workbench for fixed-point risk cockpit projections, attributable accounting movements, stable alerts, and explicit-time schedule planning.
- **Controlled-Live Safety Kernel:** Opaque credential references, zeroizing secret-material boundary, time-bounded four-eyes activations and approvals, and disaster-recovery status capabilities.

---

## System Constraints and Boundaries

### Architectural Constraints
- **Strict Data Flow:** The non-negotiable flow must be followed: `market event -> strategy -> order intent -> risk decision -> OMS -> simulator/broker edge -> execution -> portfolio update -> immutable audit event -> evidence UI`.
- **Declarative Intents:** An intent is strictly declarative. Strategies must never call an adapter, access credentials, or bypass the risk kernel. Core domain does not depend on delivery frameworks.
- **Deterministic Evidence Model:** Replay must remain deterministic (identical data, configuration, clock, ordering, and seed must yield identical results). No hidden wall-clock behavior, implicit randomness, or lossy numeric conversion is allowed in trading paths.
- **Explicit State Uncertainty:** Duplicate or late broker evidence, disconnects, and reconciliation scenarios must not create silent loss or duplicate fills. Ambiguous state remains `UNKNOWN` until authoritative evidence resolves it.

### Initial Release Boundary
- **Asset Classes:** Initially limited to US equities and ETFs.
- **Brokers:** Single integration via the Interactive Brokers (IBKR) API.
- **Accounts:** Single account support.
- **Trading Modes:** Paper trading followed by limited-capital live trading.
- **Order Types:** Restricted to Market, Limit, Stop, and Bracket orders.
- **Strategy Horizons:** End-of-day and intraday strategies.

### Out of Scope (Currently Unsupported)
- High-frequency trading (HFT)
- Custody services
- Investment advice
- Unrestricted data redistribution
- Mobile applications
- Social trading features
- Multiple broker integrations
