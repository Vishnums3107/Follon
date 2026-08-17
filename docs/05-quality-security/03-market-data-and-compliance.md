# Market-data and compliance posture

## Market data

Market data is both a technical and licensing concern. The initial commercial release should use one of these models:

1. Customer brings their own entitlement.
2. Broker-provided data attached to the customer's account.
3. Customer contracts directly with a data vendor.

Do not redistribute controlled exchange data or become an authorized distributor until the business can support legal review, subscriber administration, reporting, and exchange fees.

## Initial operating model

- Users own and control their brokerage accounts.
- The product connects with user authorization but does not custody assets or pool money.
- Users define and approve strategies and limits.
- The product records user actions and automation.
- Marketing makes no return or performance promises.
- The product does not rank public strategies or give personalized securities recommendations.

## Required advice before public live trading

Obtain counsel covering adviser/research-analyst classification, broker/API terms, automated-trading rules, data licensing, privacy, cybersecurity, record retention, disclosures, tax, and cross-border restrictions. An India launch requires a separately designed, broker- and counsel-reviewed regulatory workflow.

## India retail-algo status

SEBI Circular `SEBI/HO/MIRSD/MIRSD-PoD/P/CIR/2025/132`, dated 2025-09-30,
states that the retail-algo framework in the 2025-02-04 circular, its
implementation standards, and exchange operational modalities apply to all
stock brokers from 2026-04-01. This repository therefore treats India-facing
API algo distribution as regulated now, even though India product support is a
later release.

Before any India-facing pilot, re-check the current SEBI circulars and the
selected exchange and broker rules, then retain dated legal and broker sign-off
for the exact operating model. Documentation here is an engineering control,
not legal advice. Primary source: [SEBI's 2025-09-30 implementation-timeline
circular](https://www.sebi.gov.in/legal/circulars/sep-2025/extension-of-timeline-for-implementation-of-sebi-circular-dated-february-04-2025-on-safer-participation-of-retail-investors-in-algorithmic-trading-_96979.html).
