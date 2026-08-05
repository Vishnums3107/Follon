# UX and primary screens

## Safety principles

- Risk, exposure, drawdown, and system health are more prominent than projected returns.
- The UI clearly separates signal, intent, risk approval, broker submission, acknowledgement, execution, and portfolio effect.
- Paper and live modes are visually unmistakable.
- Every rejection explains the exact rule, inputs, thresholds, actual value, and policy version.

Live mode requires explicit account selection, a persistent environment indicator and risk-limit display, confirmation for dangerous actions, and (where implemented) passkey or hardware approval for global-limit changes.

## Primary screens

| Screen | Primary purpose |
| --- | --- |
| Command Center | System, broker, strategy, and risk status |
| Research Lab | Datasets, notebooks, experiments |
| Strategy Studio | Strategy configuration, versioning, deployment |
| Backtest Explorer | Results, trades, regimes, sensitivity |
| Execution Blotter | Intents, orders, fills, rejections |
| Risk Cockpit | Exposure, limits, drawdown, kill switches |
| Portfolio | Positions, attribution, scenarios |
| Replay and Incidents | Production reconstruction and incident timeline |
| Journal | Decisions, annotations, review |
| Administration | Credentials, accounts, permissions, audit |

## First UI deliverable

Build only the desktop view needed to display the vertical-slice event trail and current simulated position/P&L. Do not start charts, indicators, or the complete screen catalogue before the system can explain the first simulated order.
