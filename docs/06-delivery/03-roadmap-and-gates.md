# Roadmap and gates

| Period | Outcome | Exit gate |
| --- | --- | --- |
| Months 0–2 | Product/domain foundation, repository/CI, event/instrument/calendar models, desktop shell | Historical events ingest, persist, and replay deterministically |
| Months 3–5 | Importer, bar builder, corporate actions, SDK, backtester, accounting, reports, experiment metadata | Versioned strategy + data + config produce identical results repeatedly |
| Months 6–8 | OMS, risk, IBKR paper adapter, reconciliation, kill switches, reconnect, fault-injection broker, dashboard | 30 paper trading days with no unexplained order/position discrepancy |
| Months 9–11 | Controlled live account, credential security, approvals, shadow, canary, incident/DR, monitoring | 60 small-capital live days with complete auditability and no unresolved accounting discrepancy |
| Months 12–14 | Risk cockpit, attribution, journal, alerts, scheduling, parameter/config tools, replay UI, reports | Five design partners complete normal work unaided |
| Months 15–17 | Options model, chains, Greeks, vol, multi-leg/scenario/expiry workflows | Options reconcile and reproduce across backtest, paper, and live |
| Months 18–20 | Billing, provisioning, retention/privacy, penetration test, runbooks, onboarding, signed releases, self-hosting | Ten paying professionals or three paying organizations |
| Months 21–24 | Exactly one expansion path | Measurable paid demand for the selected expansion |

## Service objectives before enterprise sales

- 100% audit coverage of trading actions.
- Risk-check p99 below 5 ms in the local core.
- No persisted order/execution-event loss after acknowledgement.
- At least 99.9% production availability.
- Ordinary control-plane recovery within 15 minutes.
- Reconcile automatically after every reconnect and verify critical backup restore daily.
