# Documentation index

The master plan has been decomposed into small, implementation-oriented documents. Each document owns one decision area and should be updated as that decision changes. Architecture decisions belong in `02-architecture/adr/`; operational procedures should be added under `operations/` when implementation begins.

Use the [source map](00-source-map.md) to locate every section of the original plan.

| Area | Use it for |
| --- | --- |
| [00-product](00-product/) | Product promise, customer, scope, rollout, and commercial boundary |
| [01-domain](01-domain/) | Ubiquitous language and stable domain contracts |
| [02-architecture](02-architecture/) | System boundaries, storage, protocols, repository ownership, and ADRs |
| [03-capabilities](03-capabilities/) | Behavioural requirements for the trading kernel |
| [04-experience](04-experience/) | UX safety rules and primary screens |
| [05-quality-security](05-quality-security/) | Reliability, testing, security, market-data, and compliance requirements |
| [06-delivery](06-delivery/) | Build sequence, first vertical slice, and solo-founder working cadence |
| [07-issues](07-issues/) | External review findings and their resolution status |
| [operations](operations/) | Operational controls and implementation-era records |

## Reading order for the first implementation

1. [Product charter](00-product/01-product-charter.md)
2. [Scope and rollout](00-product/02-scope-and-rollout.md)
3. [Domain glossary](01-domain/01-glossary.md)
4. [Event envelope](01-domain/02-event-envelope.md)
5. [Workflow and order intent](01-domain/04-workflow-and-order-intent.md)
6. [System architecture](02-architecture/01-system-architecture.md)
7. [First vertical slice](06-delivery/02-first-vertical-slice.md)
8. [Quality gate](06-delivery/01-foundation-readiness.md)
9. [Implementation status](06-delivery/05-implementation-status.md)
10. [Months 3–5 status](06-delivery/06-months-3-5-status.md)

11. [Months 6–8 status](06-delivery/07-months-6-8-status.md)

12. [Months 9–11 status](06-delivery/08-months-9-11-status.md)

13. [Controlled-live runbook](operations/02-controlled-live-runbook.md)

14. [Months 12–14 status](06-delivery/09-months-12-14-status.md)

15. [Operator workbench runbook](operations/03-operator-workbench-runbook.md)

16. [Months 15–17 status](06-delivery/10-months-15-17-status.md)

17. [Months 18–20 status](06-delivery/11-months-18-20-status.md)

18. [Commercial and self-hosting runbook](operations/04-commercial-self-hosting-runbook.md)

19. [Privacy and retention runbook](operations/05-privacy-retention-runbook.md)

20. [Step-by-step implementation matrix](06-delivery/13-step-by-step-implementation-matrix.md)

## Document conventions

- Use UTC for stored event times; retain exchange-local context where relevant.
- Use canonical IDs, never tickers, as permanent identifiers.
- Changes to an accepted architecture decision require a new ADR that supersedes the old one.
- A requirement marked **invariant** must be covered by an automated test before the relevant feature is complete.
