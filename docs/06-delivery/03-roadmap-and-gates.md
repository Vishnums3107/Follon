# Roadmap and gates

The periods below are sizing labels retained from the master plan. They are not
calendar commitments, a valuation schedule, or evidence that one founder can
complete the full system in 24 elapsed months. Work advances by observed exit
gate. A later implementation may be explored, but it remains a prototype and
must not expand the marketed, deployed, or capital-bearing scope while an
earlier gate is open.

| Reference window | Outcome | Exit gate |
| --- | --- | --- |
| Months 0–2 | Product/domain foundation, repository/CI, event/instrument/calendar models, desktop shell | Historical events ingest, persist, and replay deterministically |
| Months 3–5 | Importer, bar builder, corporate actions, SDK, backtester, accounting, reports, experiment metadata | Versioned strategy + data + config produce identical results repeatedly |
| Months 6–8 | OMS, risk, IBKR paper adapter, reconciliation, kill switches, reconnect, fault-injection broker, dashboard | 30 paper trading days with no unexplained order/position discrepancy |
| Months 9–11 | Controlled live account, credential security, approvals, shadow, canary, incident/DR, monitoring | 60 small-capital live days with complete auditability and no unresolved accounting discrepancy |
| Months 12–14 | Risk cockpit, attribution, journal, alerts, scheduling, parameter/config tools, replay UI, reports | Five design partners complete normal work unaided |
| Months 15–17 | Options model, chains, Greeks, vol, multi-leg/scenario/expiry workflows | Options reconcile and reproduce across backtest, paper, and live |
| Months 18–20 | Billing, provisioning, retention/privacy, penetration test, runbooks, onboarding, signed releases, self-hosting | Ten paying professionals or three paying organizations |
| Months 21–24 | Exactly one expansion path | Measurable paid demand for the selected expansion |

## Gate enforcement

At the evidence snapshot dated 2026-08-13, the non-live research gate has
passed, while the repository records **0/30** observed paper sessions,
**0/60** controlled-live sessions, **0/5** unaided design partners, **0**
independent broker-backed options reconciliation sessions, and no paying-
customer evidence. Consequently:

- The active execution scope is the Release 1 replay-to-paper workflow plus
  founder-led customer validation.
- New brokers, asset classes, India order flow, FIX, multi-account allocation,
  team features, and additional commercial infrastructure are frozen.
- Existing later-phase code is reusable technical evidence only. It does not
  satisfy an operational, customer, compliance, or revenue gate.
- A gate changes status only when its independently retained evidence is linked
  from the relevant implementation-status document. Passing tests proves the
  mechanism, not the real-world outcome.
- If customer evidence invalidates the Release 1 workflow, narrow or stop it
  before resuming platform expansion.

The plan should be re-estimated after every gate using observed throughput,
support load, defect rate, and founder runway. If a gate needs concurrent
engineering, security/compliance, and sales ownership that exceeds available
capacity, reduce scope or staff it explicitly; do not preserve the date by
silently weakening the gate.

## Service objectives before enterprise sales

- 100% audit coverage of trading actions.
- Risk-check p99 below 5 ms in the local core.
- No persisted order/execution-event loss after acknowledgement.
- At least 99.9% production availability.
- Ordinary control-plane recovery within 15 minutes.
- Reconcile automatically after every reconnect and verify critical backup restore daily.
