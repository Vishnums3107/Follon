# Personal trading mandate template

**Status:** an unsigned planning template. Completing this document does not
authorize a PAPER or LIVE order, replace broker agreements, provide legal or
tax advice, or satisfy any acceptance gate.

Use one reviewed copy per account and strategy set. Store the signed copy in
the designated controlled evidence system; retain only its SHA-256 and a
pseudonymous canonical reference in Follon journals.

## 1. Owner and scope

- Mandate ID: `<canonical.id>`
- Owner / accountable operator: `<canonical.operator.id>`
- Effective UTC date: `<YYYY-MM-DDTHH:MM:SSZ>`
- Review cadence and next review UTC date: `<explicit cadence and timestamp>`
- Broker account reference: `<pseudonymous account ID>`
- Permitted environment: `<BACKTEST | PAPER | controlled LIVE>`
- Permitted strategies and immutable bundle hashes: `<strategy ID, version, SHA-256>`
- Permitted instruments, venues, sessions, and order types: `<approved list>`

## 2. Capital, leverage, and loss limits

Fill every field with a finite fixed-point amount or basis-point limit before
approval. The values must agree with the immutable risk-policy artifact used by
the execution system; a mismatch is a stop condition.

| Control | Mandate limit | Enforced policy/artifact reference |
| --- | ---: | --- |
| Maximum funded capital | `<amount currency>` | `<SHA-256>` |
| Maximum gross exposure | `<amount currency>` | `<SHA-256>` |
| Maximum leverage | `<bps>` | `<SHA-256>` |
| Maximum position / concentration | `<amount or bps>` | `<SHA-256>` |
| Maximum daily loss | `<amount currency>` | `<SHA-256>` |
| Maximum drawdown | `<bps>` | `<SHA-256>` |
| Maximum margin utilization | `<bps>` | `<SHA-256>` |
| Maximum open orders / order rate | `<integer>` | `<SHA-256>` |

## 3. Risk, stop, and escalation rules

1. Define every hard stop: active kill switch, broker disconnect, stale market
   data, audit/journal failure, reconciliation discrepancy, breached risk limit,
   monitoring blind spot, certificate or secret-custody failure, and unresolved
   incident. Each is a stop condition for new controlled-LIVE submissions.
2. Assign a named operator and independent reviewer for any activation,
   material parameter change, model promotion/demotion, and release promotion.
   The same person must not supply both mandatory approvals.
3. State the maximum time allowed to reconcile orders, fills, positions, cash,
   margin, and fees after disconnect or session close. Do not begin the next
   capital session until reconciliation is clean and retained.
4. Specify broker support and incident escalation contacts outside this
   repository. Do not place phone numbers, credentials, secret references, or
   personal data in an operations journal.

## 4. Model and execution governance

- Required backtest/replay artifact and data/strategy/config hashes:
  `<references>`
- Required pre-deployment review and independent validation criteria:
  `<criteria>`
- Permitted EMS algorithms, benchmark, participation/urgency, price collars,
  and cancellation behavior: `<controls>`
- Model-risk decision rule: `<PROMOTE | DEMOTE | HOLD criteria>`
- Execution-cost review threshold and response: `<TCA threshold and action>`
- Required game-day cadence and fault scenarios: `<schedule and scenario IDs>`

Record completed decisions with `follon-operations model-risk-record` and
completed fault exercises with `follon-operations game-day-record`. Their
artifact hashes must point to independently retained evidence, not a prose
assertion in a journal field.

## 5. End-of-day and monthly review

At each session close, preserve the immutable operations report, transaction
cost analysis artifact, broker statement/export hash, reconciliation result,
and any incident record. Review P&L attribution, costs, allocation/fills,
margin, stale data, and all exceptions before the next capital session.

At the defined monthly cadence, review: performance against mandate limits,
strategy/model changes, execution-cost trends, broker statements, margin/FX,
open incidents, access/secret ownership, backups/restore evidence, and whether
the mandate itself requires amendment. An amendment requires a new mandate ID,
new effective time, and new independent approval; never edit a signed record.

## 6. Approval

| Role | Canonical approver ID | Evidence/approval artifact SHA-256 | Approved at UTC |
| --- | --- | --- | --- |
| Accountable operator | `<id>` | `<SHA-256>` | `<UTC>` |
| Independent reviewer | `<id>` | `<SHA-256>` | `<UTC>` |
| Legal/compliance reviewer, where required | `<external authority>` | `<SHA-256>` | `<UTC>` |

No LIVE capital operation is authorized unless the required external legal,
broker, security, operational, and acceptance gates in the conformance audit
are also complete.
