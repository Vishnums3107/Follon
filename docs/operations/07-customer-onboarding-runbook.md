# Customer onboarding runbook

This procedure creates an auditable customer boundary without treating a local
configuration file as proof of a sale. Complete contractual, tax, privacy,
identity, and payment-provider steps in the organization-approved systems
before recording any entitlement here.

## Before activation

1. Assign a canonical tenant and workspace ID. Record the customer’s identity
   only in the approved CRM/identity system, not Follon evidence artifacts.
2. Obtain the customer's plan, data-processing terms, retention policy,
   authorized contacts, support path, and (when relevant) self-host deployment
   owner. Capture consent/contract references outside the repository.
3. Confirm payment-provider evidence or a documented contract/exception. Store
   a pseudonymous external customer reference and SHA-256 evidence digest in a
   `commercial-subscription` input.
4. Provision the tenant once, append subscription evidence once per provider
   event, and independently derive the entitlement at the intended activation
   time. Do not activate if output is not `FULL`.
5. For self-hosting, complete every step in the [self-hosting runbook](04-commercial-self-hosting-runbook.md), including successful signed-release
   readiness evidence, backup/restore drill, managed-secret-helper review, and
   ownership transfer of the encrypted storage volume.

## First normal workflow

Guide the customer through one bounded normal workflow using non-sensitive
training data: import/replay, inspect evidence, run a backtest or paper status,
review a report, and locate the support/escalation path. Record completion,
blocking usability feedback, and the evidence artifact hashes in the authorized
customer-success system. Do not collect strategy source, market data, broker
credentials, or customer portfolio data in a feedback ticket.

## Operating checklist

- Re-derive entitlement at each hosted login/session policy boundary and before
  any mutable operation. `READ_ONLY` preserves evidence access but blocks new
  mutation/export; `DENIED` blocks all tenant access.
- Review the commercial ledger and customer-facing service entitlement enforcement
  monthly. Reconcile billing exceptions, grace dates, cancelled subscriptions,
  and access logs.
- Execute retention and privacy requests only through reviewed, immutable plans
  and receipts. Maintain an up-to-date inventory for all customer data stores
  and backups.
- Route security, availability, accounting, or privacy events through the
  incident and controlled-live procedures; customer success is not an incident
  substitute.

## Adoption evidence gate

The Months 18–20 commercial gate is met only when either ten paying
professionals or three paying organizations complete normal work unaided. Keep
an aggregate, privacy-minimized evidence register with contract/payment proof
location, entitlement hash, onboarding completion, normal-work completion,
support outcome, and unresolved-risk status for each participating customer.
Count a customer only after those records are independently reviewed. This
repository cannot—and does not—manufacture that evidence.
