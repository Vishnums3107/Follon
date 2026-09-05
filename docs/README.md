# Documentation index

The original `Solo Trading Operating System Master Plan.pdf` is the source
artifact. It has been decomposed into small, implementation-oriented documents
below. Each document owns one decision area and should be updated as that
decision changes.

- Locate any section of the original plan with the
  [master-plan source map](00-source-map.md).
- Architecture decisions live in `02-architecture/adr/`; operational procedures
  live under `operations/` once implementation begins.

- [End-to-end product plan](06-delivery/15-end-to-end-product-plan.md) ? current implementation, remaining integrations, differentiated product proposals, and acceptance gates.

## Source artifacts

| Artifact | What it is |
| --- | --- |
| [Solo Trading Operating System Master Plan.pdf](Solo%20Trading%20Operating%20System%20Master%20Plan.pdf) | The original, undecomposed master plan. Source of truth for intent; maintained decisions live in the documents below. |
| [Master-plan source map](00-source-map.md) | Maps each section of the PDF to the document that now maintains it. |

## Areas and documents

### 00-product — product promise, customer, scope, rollout, commercial
- [Product charter](00-product/01-product-charter.md) — what the system is, constraints, and the reference-architecture classification.
- [Scope and rollout](00-product/02-scope-and-rollout.md) — what is in/out and the market rollout plan.
- [Customer, value, and commercial model](00-product/03-customer-value-and-commercial.md) — target customer, value, and commercial boundary.
- [Features and constraints](00-product/04-features-and-constraints.md) — core functionalities, boundaries, and release scope.

### 01-domain — ubiquitous language and stable domain contracts
- [Domain glossary](01-domain/01-glossary.md) — canonical terms and IDs.
- [Event envelope](01-domain/02-event-envelope.md) — the shared event/audit record shape.
- [Instrument model](01-domain/03-instrument-model.md) — how tradable instruments are modelled.
- [Workflow and order intent](01-domain/04-workflow-and-order-intent.md) — order lifecycle and intent modelling.

### 02-architecture — boundaries, storage, protocols, ownership, ADRs
- [System architecture](02-architecture/01-system-architecture.md) — system boundaries and components.
- [Storage and protocols](02-architecture/02-storage-and-protocols.md) — persistence and wire protocols.
- [Repository guide](02-architecture/03-repository-guide.md) — repository ownership and layout.
- [ADR 0001: modular monolith](02-architecture/adr/0001-modular-monolith.md) — accepted architecture decision.
- [ADR 0002: deterministic FX core](02-architecture/adr/0002-deterministic-fx-core.md) — value-dated FX pricing boundary.
- [ADR 0003: execution plan evidence](02-architecture/adr/0003-execution-plan-evidence.md) — execution-plan evidence and capability routing.

### rfcs — request for comments and phased delivery
- [RFC 0001: phased multi-asset platform delivery](rfcs/0001-institutional-platform-phased-delivery.md)
- [RFC 0002: deterministic FX pricing and risk contracts](rfcs/0002-fx-pricing-risk.md)
- [RFC 0003: execution-plan contracts, capability routing, and TCA evidence](rfcs/0003-execution-plan-evidence.md)

### 03-capabilities — behavioural requirements for the trading kernel
- [Market data and replay](03-capabilities/01-market-data-and-replay.md)
- [Strategy SDK and backtesting](03-capabilities/02-strategy-sdk-and-backtesting.md)
- [Order management and execution](03-capabilities/03-oms-and-execution.md)
- [Pre-trade risk engine](03-capabilities/04-pre-trade-risk.md)
- [Portfolio, audit, and reconciliation](03-capabilities/05-portfolio-audit-and-reconciliation.md)

### 04-experience — UX safety rules and primary screens
- [UX and primary screens](04-experience/01-ux-and-primary-screens.md)
- [UI overhaul audit — preflight](04-experience/02-ui-overhaul-audit.md) — current desktop UI architecture, design constraints, and baseline-capture gate.
- [UI overhaul task list](04-experience/03-ui-overhaul-task-list.md) — visible redesign sequence and verification checklist.

### 05-quality-security — reliability, testing, security, compliance
- [Reliability and testing](05-quality-security/01-reliability-and-testing.md)
- [Security architecture](05-quality-security/02-security-architecture.md)
- [Market-data and compliance posture](05-quality-security/03-market-data-and-compliance.md)

### 06-delivery — build sequence, status, and solo-founder cadence
- [Foundation readiness](06-delivery/01-foundation-readiness.md) — first-30-days quality gate.
- [First vertical slice](06-delivery/02-first-vertical-slice.md) — narrow starting scope.
- [Roadmap and gates](06-delivery/03-roadmap-and-gates.md) — evidence-gated reference sequence.
- [Solo-founder operating system](06-delivery/04-solo-founder-operating-system.md) — founder working cadence and runway.
- [Implementation status](06-delivery/05-implementation-status.md) — requirement-by-requirement verdict.
- [Months 3–5 status](06-delivery/06-months-3-5-status.md)
- [Months 6–8 status](06-delivery/07-months-6-8-status.md)
- [Months 9–11 status](06-delivery/08-months-9-11-status.md)
- [Months 12–14 status](06-delivery/09-months-12-14-status.md)
- [Months 15–17 status](06-delivery/10-months-15-17-status.md)
- [Months 18–20 status](06-delivery/11-months-18-20-status.md)
- [Dashboard feature integration status](06-delivery/12-dashboard-feature-integration-status.md)
- [Step-by-step implementation matrix](06-delivery/13-step-by-step-implementation-matrix.md)
- [Master-plan conformance audit](06-delivery/14-master-plan-conformance-audit.md)

### 07-issues — external review and resolution
- [System review and resolution record](07-issues/issues-in-system.md) — original review plus resolution notes.

### operations — operational controls and implementation-era records
See [operations/README.md](operations/README.md) for the full list.
- [Foundation threat model](operations/01-foundation-threat-model.md)
- [Controlled-live canary runbook](operations/02-controlled-live-runbook.md)
- [Non-live research backtest deployment](operations/02-research-backtest-deployment.md)
- [Operator workbench runbook](operations/03-operator-workbench-runbook.md)
- [Commercial controls and self-hosting runbook](operations/04-commercial-self-hosting-runbook.md)
- [Privacy and retention runbook](operations/05-privacy-retention-runbook.md)
- [Penetration-test runbook](operations/06-penetration-test-runbook.md)
- [Customer onboarding runbook](operations/07-customer-onboarding-runbook.md)
- [Dashboard deployment runbook](operations/08-dashboard-deployment-runbook.md)
- [Production operations and evidence runbook](operations/09-production-operations-runbook.md)

> Note: `operations/` currently has two files sharing the `02-` prefix
> (`02-controlled-live-runbook.md` and `02-research-backtest-deployment.md`).
> They are distinct documents; renumber one if you later restructure.

## Recommended reading order

**Start here (foundational):**
1. [Product charter](00-product/01-product-charter.md)
2. [Domain glossary](01-domain/01-glossary.md)
3. [Event envelope](01-domain/02-event-envelope.md)
4. [System architecture](02-architecture/01-system-architecture.md)
5. [First vertical slice](06-delivery/02-first-vertical-slice.md)

**Build sequence:**
6. [Foundation readiness](06-delivery/01-foundation-readiness.md)
7. [Roadmap and gates](06-delivery/03-roadmap-and-gates.md)
8. [Solo-founder operating system](06-delivery/04-solo-founder-operating-system.md)
9. [Implementation status](06-delivery/05-implementation-status.md)
10. Monthly status: [3–5](06-delivery/06-months-3-5-status.md) → [6–8](06-delivery/07-months-6-8-status.md) → [9–11](06-delivery/08-months-9-11-status.md) → [12–14](06-delivery/09-months-12-14-status.md) → [15–17](06-delivery/10-months-15-17-status.md) → [18–20](06-delivery/11-months-18-20-status.md)
11. [Dashboard feature integration status](06-delivery/12-dashboard-feature-integration-status.md)
12. [Step-by-step implementation matrix](06-delivery/13-step-by-step-implementation-matrix.md)
13. [Master-plan conformance audit](06-delivery/14-master-plan-conformance-audit.md)

**Operations (in deployment order):**
14. [Foundation threat model](operations/01-foundation-threat-model.md)
15. [Non-live research backtest deployment](operations/02-research-backtest-deployment.md)
16. [Controlled-live canary runbook](operations/02-controlled-live-runbook.md)
17. [Operator workbench runbook](operations/03-operator-workbench-runbook.md)
18. [Commercial controls and self-hosting runbook](operations/04-commercial-self-hosting-runbook.md)
19. [Privacy and retention runbook](operations/05-privacy-retention-runbook.md)
20. [Penetration-test runbook](operations/06-penetration-test-runbook.md)
21. [Customer onboarding runbook](operations/07-customer-onboarding-runbook.md)
22. [Dashboard deployment runbook](operations/08-dashboard-deployment-runbook.md)
23. [Production operations and evidence runbook](operations/09-production-operations-runbook.md)

**Reference:**
24. [System review and resolution record](07-issues/issues-in-system.md)
25. [Master-plan source map](00-source-map.md) and the [original PDF](Solo%20Trading%20Operating%20System%20Master%20Plan.pdf)

## Document conventions
- Use UTC for stored event times; retain exchange-local context where relevant.
- Use canonical IDs, never tickers, as permanent identifiers.
- Changes to an accepted architecture decision require a new ADR that supersedes the old one.
- A requirement marked **invariant** must be covered by an automated test before the relevant feature is complete.
